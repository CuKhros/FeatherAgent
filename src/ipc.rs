//! 前端契约传输层（§7.2）：Unix 用 UDS，Windows 用命名管道。
//!
//! 上层只认 `Listener` / `Stream`，不关心平台。命名管道的名字由 socket 路径
//! 哈希得到，服务端与客户端用同一路径即可对上；默认 DACL 限制为同用户访问。

use std::io::{self, Read, Write};
use std::path::Path;

#[cfg(unix)]
mod imp {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};

    pub struct Listener {
        inner: UnixListener,
    }

    pub struct Stream {
        pub(crate) inner: UnixStream,
    }

    impl Listener {
        pub fn bind(path: &Path) -> io::Result<Self> {
            if path.exists() {
                let _ = std::fs::remove_file(path);
            }
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let inner = UnixListener::bind(path)?;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
            Ok(Self { inner })
        }

        pub fn accept(&mut self) -> io::Result<Stream> {
            let (s, _) = self.inner.accept()?;
            Ok(Stream { inner: s })
        }
    }

    impl Stream {
        pub fn try_clone(&self) -> io::Result<Stream> {
            Ok(Stream {
                inner: self.inner.try_clone()?,
            })
        }
        pub fn shutdown(&self, how: std::net::Shutdown) -> io::Result<()> {
            self.inner.shutdown(how)
        }
    }

    impl Read for Stream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.inner.read(buf)
        }
    }
    impl Write for Stream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.inner.write(buf)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.inner.flush()
        }
    }

    pub fn connect(path: &Path) -> io::Result<Stream> {
        Ok(Stream {
            inner: UnixStream::connect(path)?,
        })
    }

    /// 单次探测：不等待、不重连。启动存在性检查与 wait_ready 必须用它，
    /// 否则 Windows 的 3 秒重试会被计入冷启动耗时。
    pub fn try_connect(path: &Path) -> io::Result<Stream> {
        connect(path)
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{
        CloseHandle, DuplicateHandle, GetLastError, DUPLICATE_SAME_ACCESS, ERROR_BROKEN_PIPE,
        ERROR_FILE_NOT_FOUND, ERROR_IO_PENDING, ERROR_NO_DATA, ERROR_PIPE_BUSY,
        ERROR_PIPE_CONNECTED, ERROR_PIPE_NOT_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE,
        INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, ReadFile, WriteFile, FILE_FLAG_OVERLAPPED, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, WaitNamedPipeW};
    use windows_sys::Win32::System::Threading::{CreateEventW, GetCurrentProcess};
    use windows_sys::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};

    const PIPE_ACCESS_DUPLEX: u32 = 0x0000_0003;
    const PIPE_TYPE_BYTE: u32 = 0x0000_0000;
    const PIPE_READMODE_BYTE: u32 = 0x0000_0000;
    const PIPE_WAIT: u32 = 0x0000_0000;
    const PIPE_UNLIMITED_INSTANCES: u32 = 255;
    /// Windows 7+：拒绝远程客户端，只允许本机连接。
    const PIPE_REJECT_REMOTE_CLIENTS: u32 = 0x0000_0008;

    struct Event(HANDLE);

    impl Event {
        fn new() -> io::Result<Self> {
            let h = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
            if h == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(h))
        }
    }

    impl Drop for Event {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    struct PipeHandle {
        handle: HANDLE,
    }

    unsafe impl Send for PipeHandle {}
    unsafe impl Sync for PipeHandle {}

    impl Drop for PipeHandle {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.handle) };
        }
    }

    pub struct Listener {
        name: Vec<u16>,
        pending: Option<HANDLE>,
    }

    pub struct Stream {
        inner: Arc<PipeHandle>,
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// 由 socket 路径派生稳定的管道名（服务端/客户端各算一次，结果一致）。
    pub fn pipe_name(path: &Path) -> String {
        // Windows 路径大小写不敏感；统一小写 + 反斜杠，避免两端写法不同算出不同管道名。
        let key = path.to_string_lossy().replace('/', "\\").to_lowercase();
        let h = blake3::hash(key.as_bytes()).to_hex();
        format!(r"\\.\pipe\featheragent-{}", &h[..16])
    }

    fn fresh_instance(name: &[u16]) -> io::Result<HANDLE> {
        let h = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                64 * 1024,
                64 * 1024,
                0,
                std::ptr::null(),
            )
        };
        if h == INVALID_HANDLE_VALUE || h == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(h)
    }

    /// 等待一个重叠 I/O 完成；句柄必须是 FILE_FLAG_OVERLAPPED 打开的。
    fn complete(handle: HANDLE, ov: &OVERLAPPED, immediate: i32, bytes: u32) -> io::Result<u32> {
        if immediate != 0 {
            return Ok(bytes);
        }
        let err = unsafe { GetLastError() };
        if err != ERROR_IO_PENDING {
            return Err(io::Error::from_raw_os_error(err as i32));
        }
        let mut transferred = 0u32;
        let ok = unsafe { GetOverlappedResult(handle, ov, &mut transferred, 1) };
        if ok == 0 {
            let err = unsafe { GetLastError() };
            return Err(io::Error::from_raw_os_error(err as i32));
        }
        Ok(transferred)
    }

    /// 重叠 ConnectNamedPipe。客户端可能已经提前连上（ERROR_PIPE_CONNECTED），也算成功。
    fn connect_instance(handle: HANDLE) -> io::Result<()> {
        let ev = Event::new()?;
        let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
        ov.hEvent = ev.0;
        let ok = unsafe { ConnectNamedPipe(handle, &mut ov) };
        if ok != 0 {
            return Ok(());
        }
        let err = unsafe { GetLastError() };
        if err == ERROR_PIPE_CONNECTED {
            return Ok(());
        }
        if err != ERROR_IO_PENDING {
            return Err(io::Error::from_raw_os_error(err as i32));
        }
        let mut n = 0u32;
        let done = unsafe { GetOverlappedResult(handle, &ov, &mut n, 1) };
        if done == 0 {
            let err = unsafe { GetLastError() };
            return Err(io::Error::from_raw_os_error(err as i32));
        }
        Ok(())
    }

    impl Listener {
        pub fn bind(path: &Path) -> io::Result<Self> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let name = wide(&pipe_name(path));
            let pending = fresh_instance(&name)?;
            Ok(Self {
                name,
                pending: Some(pending),
            })
        }

        pub fn accept(&mut self) -> io::Result<Stream> {
            loop {
                let handle = match self.pending.take() {
                    Some(h) => h,
                    None => fresh_instance(&self.name)?,
                };
                match connect_instance(handle) {
                    Ok(()) => {
                        // 先补下一个监听实例，允许 TUI/WebUI/观察者并发连接。
                        // 补实例失败也不丢弃已建立的连接；下一次 accept 会再补。
                        self.pending = fresh_instance(&self.name).ok();
                        return Ok(Stream {
                            inner: Arc::new(PipeHandle { handle }),
                        });
                    }
                    Err(e) if e.raw_os_error() == Some(ERROR_NO_DATA as i32) => {
                        // 客户端连上后立刻断开：复位实例并把错误交回 accept 循环。
                        // 不能在这里无限 continue，否则 Server::stop 的唤醒连接
                        // 被当成 ERROR_NO_DATA 后，accept 会再次阻塞、stop 永远收不到。
                        unsafe { CloseHandle(handle) };
                        self.pending = fresh_instance(&self.name).ok();
                        return Err(e);
                    }
                    Err(e) => {
                        unsafe { CloseHandle(handle) };
                        self.pending = fresh_instance(&self.name).ok();
                        return Err(e);
                    }
                }
            }
        }
    }

    impl Stream {
        pub fn try_clone(&self) -> io::Result<Stream> {
            // 关键：DuplicateHandle，而不是 Arc 共享同一个 HANDLE。
            // Windows 对同步句柄的挂起 Read 会阻塞同一句柄上的 Write；
            // 独立句柄 + OVERLAPPED 才能让读写线程真正并发。
            let mut dup: HANDLE = 0;
            let ok = unsafe {
                DuplicateHandle(
                    GetCurrentProcess(),
                    self.inner.handle,
                    GetCurrentProcess(),
                    &mut dup,
                    0,
                    0,
                    DUPLICATE_SAME_ACCESS,
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Stream {
                inner: Arc::new(PipeHandle { handle: dup }),
            })
        }

        pub fn shutdown(&self, _how: std::net::Shutdown) -> io::Result<()> {
            // 命名管道不支持半关闭；由内核在连接线程结束时关闭句柄。
            Ok(())
        }
    }

    impl Read for Stream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if buf.is_empty() {
                return Ok(0);
            }
            let ev = Event::new()?;
            let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
            ov.hEvent = ev.0;
            let mut n = 0u32;
            let ok = unsafe {
                ReadFile(
                    self.inner.handle,
                    buf.as_mut_ptr() as *mut _,
                    buf.len() as u32,
                    &mut n,
                    &mut ov,
                )
            };
            match complete(self.inner.handle, &ov, ok, n) {
                Ok(n) => Ok(n as usize),
                Err(e)
                    if e.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32)
                        || e.raw_os_error() == Some(ERROR_PIPE_NOT_CONNECTED as i32) =>
                {
                    Ok(0)
                }
                Err(e) => Err(e),
            }
        }
    }

    impl Write for Stream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let mut written = 0usize;
            while written < buf.len() {
                let ev = Event::new()?;
                let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
                ov.hEvent = ev.0;
                let mut n = 0u32;
                let ok = unsafe {
                    WriteFile(
                        self.inner.handle,
                        buf[written..].as_ptr() as *const _,
                        (buf.len() - written) as u32,
                        &mut n,
                        &mut ov,
                    )
                };
                let n = complete(self.inner.handle, &ov, ok, n)?;
                if n == 0 {
                    break;
                }
                written += n as usize;
            }
            Ok(written)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn connect_once(name: &[u16]) -> io::Result<Stream> {
        let h = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                0,
            )
        };
        if h != INVALID_HANDLE_VALUE && h != 0 {
            return Ok(Stream {
                inner: Arc::new(PipeHandle { handle: h }),
            });
        }
        Err(io::Error::last_os_error())
    }

    /// 单次探测：不等待命名管道重建窗口。所有“判断是否已有内核”的场景
    /// 都必须走这里，不能复用带 3 秒 deadline 的 connect。
    pub fn try_connect(path: &Path) -> io::Result<Stream> {
        connect_once(&wide(&pipe_name(path)))
    }

    pub fn connect(path: &Path) -> io::Result<Stream> {
        let name = wide(&pipe_name(path));
        // WebUI/TUI 不应无限等待：3s 内没有可用实例就明确失败，交给上层打印。
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match connect_once(&name) {
                Ok(stream) => return Ok(stream),
                Err(e) => {
                    let err = e.raw_os_error().unwrap_or(0) as u32;
                    if err == ERROR_PIPE_BUSY
                        || err == ERROR_FILE_NOT_FOUND
                        || err == ERROR_PIPE_NOT_CONNECTED
                    {
                        let now = Instant::now();
                        if now >= deadline {
                            return Err(io::Error::new(
                                io::ErrorKind::TimedOut,
                                format!("named pipe unavailable: win32={err}"),
                            ));
                        }
                        if err == ERROR_PIPE_BUSY {
                            let wait = (deadline - now).as_millis().max(1) as u32;
                            unsafe { WaitNamedPipeW(name.as_ptr(), wait) };
                        } else {
                            // Windows 命名管道在监听端创建/重建的窗口里会短暂返回
                            // ERROR_FILE_NOT_FOUND(2) 或 ERROR_PIPE_NOT_CONNECTED(233)；
                            // 与 busy 一样退避重试，避免 bench 的 wait_ready 与 connect 之间漏接。
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        continue;
                    }
                    return Err(e);
                }
            }
        }
    }
}

#[cfg(windows)]
pub use imp::pipe_name;
pub use imp::{connect, try_connect, Listener, Stream};

/// 前端可读的实际端点：Unix 是 socket 路径，Windows 是 `\\.\pipe\...`。
pub fn endpoint(path: &Path) -> String {
    #[cfg(windows)]
    {
        imp::pipe_name(path)
    }
    #[cfg(unix)]
    {
        path.to_string_lossy().into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn try_connect_does_not_enter_the_retry_deadline() {
        let dir = std::env::temp_dir().join(format!(
            "featheragent-ipc-probe-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("missing.sock");
        let start = Instant::now();
        assert!(try_connect(&path).is_err());
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "单次探测不应等待重试 deadline: {:?}",
            start.elapsed()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
