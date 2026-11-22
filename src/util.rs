//! 无依赖小工具：命令行解析、随机 token、字节格式化、SHA-1/Base64（WS 握手用）。
//!
//! 刻意不引入 clap / rand / serde_yaml 等，控制内核体积（§2.3）。

use std::io::Read;
use std::path::PathBuf;

/// 极简参数解析：支持 `--k v`、`--k=v`、`-h`。
#[derive(Debug, Default, Clone)]
pub struct Args {
    pub positional: Vec<String>,
    pub flags: Vec<(String, Option<String>)>,
}

impl Args {
    pub fn parse<I: IntoIterator<Item = String>>(it: I) -> Args {
        let mut a = Args::default();
        let mut iter = it.into_iter().peekable();
        while let Some(tok) = iter.next() {
            if let Some(rest) = tok.strip_prefix("--") {
                if let Some(eq) = rest.find('=') {
                    a.flags
                        .push((rest[..eq].to_string(), Some(rest[eq + 1..].to_string())));
                } else if iter.peek().map(|n| !n.starts_with('-')).unwrap_or(false) {
                    let v = iter.next().unwrap();
                    a.flags.push((rest.to_string(), Some(v)));
                } else {
                    a.flags.push((rest.to_string(), None));
                }
            } else {
                a.positional.push(tok);
            }
        }
        a
    }

    pub fn flag(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .rev()
            .find(|(k, _)| k == name)
            .and_then(|(_, v)| v.as_deref())
    }

    pub fn has(&self, name: &str) -> bool {
        // 传入的 name 可能带 "--" 前缀（调用方习惯写成 --flag），统一去掉后再匹配
        let name = name.strip_prefix("--").unwrap_or(name);
        self.flags.iter().any(|(k, _)| k == name) || self.positional.iter().any(|p| p == name)
    }

    pub fn path(&self, name: &str) -> Option<PathBuf> {
        self.flag(name).map(PathBuf::from)
    }
}

/// 从 `/dev/urandom` 取随机字节，用于 GUI 一次性 token。
pub fn random_token(nbytes: usize) -> String {
    let mut buf = vec![0u8; nbytes];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut buf);
    }
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn human_bytes(n: u64) -> String {
    const U: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{v:.2} {}", U[i])
}

/// SHA-1，仅用于 WebSocket 握手（RFC 6455）。
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

pub fn base64(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let v = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(v >> 18) as usize & 63] as char);
        out.push(T[(v >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(v >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[v as usize & 63] as char } else { '=' });
    }
    out
}

/// 最近秩百分位：P95 取第 ceil(0.95*n) 个（1 起算）。
pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((sorted.len() as f64) * p).ceil() as usize;
    let idx = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[idx]
}

/// 跨平台原子替换：Windows 上 `rename` 不能覆盖已存在目标，
/// 用 `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`；Unix 仍是 `rename(2)`。
pub fn replace_file(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING};
        let f: Vec<u16> = from.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let t: Vec<u16> = to.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let ok = unsafe { MoveFileExW(f.as_ptr(), t.as_ptr(), MOVEFILE_REPLACE_EXISTING) };
        if ok != 0 {
            return Ok(());
        }
        return Err(std::io::Error::last_os_error());
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(from, to)
    }
}

pub fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_parse() {
        let a = Args::parse(vec!["serve".into(), "--sync".into(), "never".into(), "--port=1".into(), "-h".into()]);
        assert_eq!(a.positional, vec!["serve".to_string(), "-h".to_string()]);
        assert_eq!(a.flag("sync"), Some("never"));
        assert_eq!(a.flag("port"), Some("1"));
    }

    #[test]
    fn sha1_base64_known_vectors() {
        assert_eq!(base64(b"abc"), "YWJj");
        assert_eq!(base64(b"ab"), "YWI=");
        let d = sha1(b"abc");
        assert_eq!(hex20(&d), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    fn hex20(b: &[u8; 20]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn percentile_works() {
        let v: Vec<f64> = (1..=100).map(|x| x as f64).collect();
        assert_eq!(percentile(&v, 0.95), 95.0);
        assert_eq!(percentile(&v, 0.5), 50.0);
    }
}
