//! 极简 HTTP/1.1 客户端（仅 http，不引 reqwest/tokio，控内核与插件体积）。
//!
//! 支持：Content-Length、chunked、连接关闭三种响应体；NDJSON / SSE 逐行流式读取。
//! HTTPS 明确不支持——本地推理运行时默认是 `http://127.0.0.1`。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Url {
    pub host: String,
    pub port: u16,
    pub path: String,
}

pub fn parse_url(s: &str) -> Result<Url, String> {
    let s = s.trim();
    if s.starts_with("https://") {
        return Err("仅支持 http://（本地运行时）；HTTPS 未实现".into());
    }
    let rest = s.strip_prefix("http://").ok_or("URL 必须以 http:// 开头")?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().map_err(|_| "端口非法")?),
        None => (authority.to_string(), 80),
    };
    if host.is_empty() {
        return Err("缺少 host".into());
    }
    Ok(Url {
        host,
        port,
        path: path.to_string(),
    })
}

fn connect(u: &Url) -> Result<TcpStream, String> {
    // 直接 TCP，不读取 HTTP_PROXY/HTTPS_PROXY；本地 Ollama 不会被系统代理截走。
    // 需要代理的远程模型请显式使用可代理的传输层，不要依赖环境变量。
    let addr = format!("{}:{}", u.host, u.port);
    let stream = TcpStream::connect(&addr).map_err(|e| format!("连接 {addr} 失败: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(300))).ok();
    Ok(stream)
}

fn build(host: &str, path: &str, body: &[u8], stream: bool) -> Vec<u8> {
    let mut req = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccept: {}\r\nConnection: {}\r\n\r\n",
        body.len(),
        if stream { "text/event-stream" } else { "application/json" },
        if stream { "keep-alive" } else { "close" }
    )
    .into_bytes();
    req.extend_from_slice(body);
    req
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn json(&self) -> Result<serde_json::Value, String> {
        serde_json::from_slice(&self.body).map_err(|e| format!("响应非 JSON: {e}"))
    }
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }
}

struct Head {
    status: u16,
    headers: HashMap<String, String>,
}

fn read_head<R: BufRead>(r: &mut R) -> Result<Head, String> {
    let mut line = String::new();
    r.read_line(&mut line).map_err(|e| e.to_string())?;
    let status: u16 = line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("非法状态行: {line:?}"))?;
    let mut headers = HashMap::new();
    loop {
        let mut h = String::new();
        r.read_line(&mut h).map_err(|e| e.to_string())?;
        if h == "\r\n" || h == "\n" || h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.insert(k.trim().to_lowercase(), v.trim().to_string());
        }
    }
    Ok(Head { status, headers })
}

fn read_exact_body<R: BufRead>(
    r: &mut R,
    headers: &HashMap<String, String>,
) -> Result<Vec<u8>, String> {
    if let Some(enc) = headers.get("transfer-encoding") {
        if enc.contains("chunked") {
            return read_chunked(r);
        }
    }
    if let Some(len) = headers
        .get("content-length")
        .and_then(|s| s.parse::<usize>().ok())
    {
        let mut buf = vec![0u8; len];
        r.read_exact(&mut buf).map_err(|e| e.to_string())?;
        return Ok(buf);
    }
    let mut buf = Vec::new();
    r.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

fn read_chunked<R: BufRead>(r: &mut R) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let mut size_line = String::new();
        r.read_line(&mut size_line).map_err(|e| e.to_string())?;
        let hex = size_line.trim().split(';').next().unwrap_or("");
        let size =
            usize::from_str_radix(hex, 16).map_err(|_| format!("非法 chunk 长度 {hex:?}"))?;
        if size == 0 {
            // trailers
            loop {
                let mut t = String::new();
                r.read_line(&mut t).map_err(|e| e.to_string())?;
                if t == "\r\n" || t == "\n" || t.is_empty() {
                    break;
                }
            }
            break;
        }
        let mut chunk = vec![0u8; size];
        r.read_exact(&mut chunk).map_err(|e| e.to_string())?;
        out.extend_from_slice(&chunk);
        let mut crlf = [0u8; 2];
        let _ = r.read_exact(&mut crlf);
    }
    Ok(out)
}

pub fn get_json(url: &str) -> Result<Response, String> {
    get_json_timeout(url, Duration::from_secs(300))
}

/// 模型列表探测等交互式查询必须快速失败，不能占用设置页 5 分钟。
pub fn get_json_timeout(url: &str, timeout: Duration) -> Result<Response, String> {
    let u = parse_url(url)?;
    let mut s = connect(&u)?;
    s.set_read_timeout(Some(timeout)).ok();
    let req = format!(
        "GET {} HTTP/1.1\r\nHost: {}:{}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
        u.path, u.host, u.port
    );
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut r = BufReader::new(s);
    let head = read_head(&mut r)?;
    let body = read_exact_body(&mut r, &head.headers)?;
    Ok(Response {
        status: head.status,
        headers: head.headers,
        body,
    })
}

pub fn post_json(url: &str, body: &serde_json::Value) -> Result<Response, String> {
    let u = parse_url(url)?;
    let body = serde_json::to_vec(body).map_err(|e| e.to_string())?;
    let mut s = connect(&u)?;
    s.write_all(&build(
        &format!("{}:{}", u.host, u.port),
        &u.path,
        &body,
        false,
    ))
    .map_err(|e| e.to_string())?;
    let mut r = BufReader::new(s);
    let head = read_head(&mut r)?;
    let body = read_exact_body(&mut r, &head.headers)?;
    Ok(Response {
        status: head.status,
        headers: head.headers,
        body,
    })
}

/// 逐行读取流式响应（NDJSON / SSE 均可），对 chunked 透明。
pub fn stream_post_lines<F>(
    url: &str,
    body: &serde_json::Value,
    mut on_line: F,
) -> Result<(), String>
where
    F: FnMut(&str) -> bool, // 返回 false 表示停止
{
    let u = parse_url(url)?;
    let body = serde_json::to_vec(body).map_err(|e| e.to_string())?;
    let mut s = connect(&u)?;
    s.write_all(&build(
        &format!("{}:{}", u.host, u.port),
        &u.path,
        &body,
        true,
    ))
    .map_err(|e| e.to_string())?;
    let mut r = BufReader::new(s);
    let head = read_head(&mut r)?;
    if head.status >= 400 {
        let b = read_exact_body(&mut r, &head.headers)?;
        return Err(format!(
            "HTTP {}: {}",
            head.status,
            String::from_utf8_lossy(&b)
        ));
    }
    let chunked = head
        .headers
        .get("transfer-encoding")
        .map(|e| e.contains("chunked"))
        .unwrap_or(false);
    if chunked {
        read_chunked_lines(&mut r, &mut on_line)?;
    } else {
        read_plain_lines(&mut r, &mut on_line)?;
    }
    Ok(())
}

fn read_plain_lines<R: BufRead, F: FnMut(&str) -> bool>(
    r: &mut R,
    on_line: &mut F,
) -> Result<(), String> {
    let mut buf = String::new();
    loop {
        buf.clear();
        let n = r.read_line(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        if !on_line(buf.trim_end_matches(['\r', '\n'])) {
            break;
        }
    }
    Ok(())
}

fn read_chunked_lines<R: BufRead, F: FnMut(&str) -> bool>(
    r: &mut R,
    on_line: &mut F,
) -> Result<(), String> {
    // 必须以**字节**累积：HTTP chunk 边界可能切断多字节 UTF-8（中文/emoji），
    // 逐 chunk 做 from_utf8_lossy 会造成乱码。
    let mut carry: Vec<u8> = Vec::new();
    loop {
        let mut size_line = String::new();
        r.read_line(&mut size_line).map_err(|e| e.to_string())?;
        let hex = size_line.trim().split(';').next().unwrap_or("");
        if hex.is_empty() {
            break;
        }
        let size =
            usize::from_str_radix(hex, 16).map_err(|_| format!("非法 chunk 长度 {hex:?}"))?;
        if size == 0 {
            let mut t = String::new();
            loop {
                t.clear();
                r.read_line(&mut t).map_err(|e| e.to_string())?;
                if t == "\r\n" || t == "\n" || t.is_empty() {
                    break;
                }
            }
            break;
        }
        let mut chunk = vec![0u8; size];
        r.read_exact(&mut chunk).map_err(|e| e.to_string())?;
        carry.extend_from_slice(&chunk);
        while let Some(pos) = carry.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = carry.drain(..=pos).collect();
            let text = String::from_utf8_lossy(&line);
            if !on_line(text.trim_end_matches(['\r', '\n'])) {
                return Ok(());
            }
        }
        let mut crlf = [0u8; 2];
        let _ = r.read_exact(&mut crlf);
    }
    if !carry.is_empty() {
        let text = String::from_utf8_lossy(&carry);
        on_line(text.trim_end_matches(['\r', '\n']));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;
    use std::thread;

    fn serve_once(resp: &'static str) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                let _ = s.write_all(resp.as_bytes());
            }
        });
        format!("http://{addr}/x")
    }

    #[test]
    fn parses_urls() {
        let u = parse_url("http://127.0.0.1:11434/api/chat").unwrap();
        assert_eq!(u.host, "127.0.0.1");
        assert_eq!(u.port, 11434);
        assert_eq!(u.path, "/api/chat");
        assert!(parse_url("https://x").is_err());
    }

    #[test]
    fn content_length_response() {
        let url = serve_once("HTTP/1.1 200 OK\r\nContent-Length: 7\r\n\r\n{\"a\":1}");
        let r = post_json(&url, &serde_json::json!({})).unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.json().unwrap()["a"], 1);
    }

    #[test]
    fn chunked_stream_lines() {
        let url = serve_once(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n\
             7\r\n{\"a\":1}\r\n1\r\n\n\r\n7\r\n{\"b\":2}\r\n1\r\n\n\r\n0\r\n\r\n",
        );
        let mut lines = Vec::new();
        stream_post_lines(&url, &serde_json::json!({}), |l| {
            if !l.is_empty() {
                lines.push(l.to_string());
            }
            true
        })
        .unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "{\"a\":1}");
    }
}
