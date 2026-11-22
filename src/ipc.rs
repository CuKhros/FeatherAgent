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
            Ok(Stream { inner: self.inner.try_clone()? })
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
        Ok(Stream { inner: UnixStream::connect(path)? })
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::sync::Arc;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_BROKEN_PIPE, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_READ,
        GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{CreateFileW, ReadFile, WriteFile, OPEN_EXISTING};
    use windows_sys::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, WaitNamedPipeW};

    const PIPE_ACCESS_DUPLEX: u32 = 0x0000_0003;
    const PIPE_TYPE_BYTE: u32 = 0x0000_0000;
    const PIPE_READMODE_BYTE: u32 = 0x0000_0000;
    const PIPE_WAIT: u32 = 0x0000_0000;
    const PIPE_UNLIMITED_INSTANCES: u32 = 255;

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

    #[derive(Clone)]
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
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
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

    impl Listener {
        pub fn bind(path: &Path) -> io::Result<Self> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let name = wide(&pipe_name(path));
            let pending = fresh_instance(&name)?;
            Ok(Self { name, pending: Some(pending) })
        }

        pub fn accept(&mut self) -> io::Result<Stream> {
            let handle = match self.pending.take() {
                Some(h) => h,
                None => fresh_instance(&self.name)?,
            };
            let ok = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) };
            if ok == 0 {
                let err = unsafe { GetLastError() };
                // ERROR_PIPE_CONNECTED：客户端在 ConnectNamedPipe 之前就连上了
                if err != ERROR_PIPE_CONNECTED {
                    unsafe { CloseHandle(handle) };
                    // 立即补一个新实例，避免后续连接无人接
                    self.pending = fresh_instance(&self.name).ok();
                    return Err(io::Error::from_raw_os_error(err as i32));
                }
            }
            // 交给调用方前先建好下一个实例，允许并发连接
            self.pending = Some(fresh_instance(&self.name)?);
            Ok(Stream { inner: Arc::new(PipeHandle { handle }) })
        }
    }

    impl Stream {
        pub fn try_clone(&self) -> io::Result<Stream> {
            Ok(Stream { inner: self.inner.clone() })
        }
        pub fn shutdown(&self, _how: std::net::Shutdown) -> io::Result<()> {
            // 句柄由 Arc 最后一个引用关闭；这里不做半关闭（命名管道不支持）
            Ok(())
        }
    }

    impl Read for Stream {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let mut n: u32 = 0;
            let ok = unsafe {
                ReadFile(
                    self.inner.handle,
                    buf.as_mut_ptr() as *mut _,
                    buf.len() as u32,
                    &mut n,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                let err = unsafe { GetLastError() };
                if err == ERROR_BROKEN_PIPE {
                    return Ok(0);
                }
                return Err(io::Error::from_raw_os_error(err as i32));
            }
            Ok(n as usize)
        }
    }

    impl Write for Stream {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let mut written = 0usize;
            while written < buf.len() {
                let mut n: u32 = 0;
                let ok = unsafe {
                    WriteFile(
                        self.inner.handle,
                        buf[written..].as_ptr() as *const _,
                        (buf.len() - written) as u32,
                        &mut n,
                        std::ptr::null_mut(),
                    )
                };
                if ok == 0 {
                    let err = unsafe { GetLastError() };
                    return Err(io::Error::from_raw_os_error(err as i32));
                }
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

    pub fn connect(path: &Path) -> io::Result<Stream> {
        let name = wide(&pipe_name(path));
        for _ in 0..50 {
            let h = unsafe {
                CreateFileW(
                    name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    0,
                    0,
                )
            };
            if h != INVALID_HANDLE_VALUE && h != 0 {
                return Ok(Stream { inner: Arc::new(PipeHandle { handle: h }) });
            }
            let err = unsafe { GetLastError() };
            if err == ERROR_PIPE_BUSY {
                unsafe { WaitNamedPipeW(name.as_ptr(), 1000) };
                continue;
            }
            return Err(io::Error::from_raw_os_error(err as i32));
        }
        Err(io::Error::new(io::ErrorKind::TimedOut, "named pipe busy"))
    }
}

pub use imp::{connect, Listener, Stream};
