//! 无依赖小工具：命令行解析、随机 token、字节格式化、SHA-1/Base64（WS 握手用）。
//!
//! 刻意不引入 clap / rand / serde_yaml 等，控制内核体积（§2.3）。

#[cfg(unix)]
use std::io::Read;
use std::path::PathBuf;

use serde_json::Value;

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

/// 取密码学随机字节，用于 WebUI 一次性 token / 会话 ID。
/// 随机源失败必须硬失败，绝不静默回退成全 0。
fn fill_random(buf: &mut [u8]) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Security::Cryptography::ProcessPrng;
        let ok = unsafe { ProcessPrng(buf.as_mut_ptr(), buf.len()) };
        assert!(ok != 0, "ProcessPrng 失败：Windows 随机源不可用");
    }
    #[cfg(unix)]
    {
        let mut f =
            std::fs::File::open("/dev/urandom").expect("打开 /dev/urandom 失败：随机源不可用");
        f.read_exact(buf)
            .expect("读取 /dev/urandom 失败：随机源不可用");
    }
    #[cfg(not(any(windows, unix)))]
    {
        panic!("当前平台没有实现随机源");
    }
}

pub fn random_token(nbytes: usize) -> String {
    assert!(nbytes > 0, "random_token 长度必须大于 0");
    let mut buf = vec![0u8; nbytes];
    fill_random(&mut buf);
    assert!(buf.iter().any(|b| *b != 0), "随机源返回全 0");
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
        out.push(if c.len() > 1 {
            T[(v >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            T[v as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

pub fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let clean: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if clean.len() % 4 != 0 {
        return Err("base64 长度不是 4 的倍数".into());
    }
    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    for c in clean.chunks(4) {
        let mut vals = [0u8; 4];
        let mut pad = 0usize;
        for (i, b) in c.iter().enumerate() {
            vals[i] = if *b == b'=' {
                pad += 1;
                0
            } else {
                T.iter()
                    .position(|x| *x == *b)
                    .ok_or_else(|| format!("base64 含非法字符: {}", *b as char))?
                    as u8
            };
        }
        if pad > 2 {
            return Err("base64 填充字符过多".into());
        }
        let v = ((vals[0] as u32) << 18)
            | ((vals[1] as u32) << 12)
            | ((vals[2] as u32) << 6)
            | vals[3] as u32;
        out.push((v >> 16) as u8);
        if pad < 2 {
            out.push((v >> 8) as u8);
        }
        if pad < 1 {
            out.push(v as u8);
        }
    }
    Ok(out)
}

/// 最近秩百分位：P95 取第 ceil(0.95*n) 个（1 起算）。
pub fn percentile(samples: &[f64], p: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    // 不要把「已排序」当前提：调用方可能传原始样本（如 bench 的 boot_samples_ms）。
    // 内部排序保证 P50/P90 与同一组样本同源，也保证 P50 ≤ P90。
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
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
        let f: Vec<u16> = from
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let t: Vec<u16> = to
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
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

/// 把 JSON 数字或数字字符串解析为 u64。
/// 小模型常把 `{"count":5}` 写成 `{"count":"5"}`；这类容错只用于计数类参数，
/// 不放宽契约中的 integer 声明，也不接受负数/小数。
pub fn json_u64(v: &Value) -> Option<u64> {
    if let Some(n) = v.as_u64() {
        return Some(n);
    }
    let s = v.as_str()?.trim();
    if s.is_empty() || s.starts_with('-') {
        return None;
    }
    s.parse::<u64>().ok()
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
        let a = Args::parse(vec![
            "serve".into(),
            "--sync".into(),
            "never".into(),
            "--port=1".into(),
            "-h".into(),
        ]);
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
    fn random_token_is_hex_and_nonzero() {
        let t = random_token(24);
        assert_eq!(t.len(), 48);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(t.chars().any(|c| c != '0'));
        assert_ne!(random_token(24), random_token(24));
    }

    #[test]
    fn json_u64_accepts_integer_and_numeric_string() {
        assert_eq!(json_u64(&serde_json::json!(5)), Some(5));
        assert_eq!(json_u64(&serde_json::json!("5")), Some(5));
        assert_eq!(json_u64(&serde_json::json!(" 5 ")), Some(5));
        assert_eq!(json_u64(&serde_json::json!("-1")), None);
        assert_eq!(json_u64(&serde_json::json!("5.0")), None);
        assert_eq!(json_u64(&serde_json::json!("five")), None);
        assert_eq!(json_u64(&serde_json::Value::Null), None);
    }

    #[test]
    fn percentile_works_on_unsorted_samples_and_p50_le_p90() {
        let v = vec![36.9, 35.6, 39.1, 56.3, 36.5];
        assert_eq!(percentile(&v, 0.5), 36.9);
        assert_eq!(percentile(&v, 0.9), 56.3);
        assert!(percentile(&v, 0.5) <= percentile(&v, 0.9));
        let sorted: Vec<f64> = (1..=100).map(|x| x as f64).collect();
        assert_eq!(percentile(&sorted, 0.95), 95.0);
        assert_eq!(percentile(&sorted, 0.5), 50.0);
    }

    #[test]
    fn base64_roundtrip_handles_padding() {
        for s in ["", "a", "ab", "abc", "hello world"] {
            let enc = base64(s.as_bytes());
            assert_eq!(base64_decode(&enc).unwrap(), s.as_bytes());
        }
        assert!(base64_decode("a").is_err());
        assert!(base64_decode("****").is_err());
    }
}
