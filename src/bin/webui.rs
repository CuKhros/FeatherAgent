//! `featheragent-webui`：WebUI 桥接器（§7.2/§7.3）。
//!
//! - 只绑 `127.0.0.1` 的**随机端口**；
//! - 一次性 token（URL fragment 传递，不进 Referer）；
//! - 校验 `Host` 与 `Origin`，防 DNS rebinding；
//! - `--app` 启动时强制 `--user-data-dir=<root>/runtime/webui-profile`；
//! - 静态资源编译进二进制（WebUI 是独立产物，不依赖 core）。
//!
//! 浏览器 ←HTTP/SSE→ 本进程 ←UDS 契约→ 内核。

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
#[cfg(windows)]
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

use featheragent::governance::paths::{Overrides, Paths};
use featheragent::server::client::Client;
use featheragent::util::{base64, random_token, sha1, Args};

const INDEX_HTML: &str = include_str!("../../web/index.html");
const INDEX_HTML_EN: &str = include_str!("../../web/index.en.html");
const APP_JS: &str = include_str!("../../web/app.js");
const APP_JS_EN: &str = include_str!("../../web/app.en.js");
const STYLE_CSS: &str = include_str!("../../web/style.css");
const FAVICON_ICO: &[u8] = include_bytes!("../../web/favicon.ico");
const LOGO_PNG: &[u8] = include_bytes!("../../web/logo.png");
const LOGO_TILE_PNG: &[u8] = include_bytes!("../../web/logo-tile.png");

/// 英文版二进制以 `-en` 结尾；同一份源码编译出中英两个 bin target。
fn has_en_suffix(name: &str) -> bool {
    name.to_lowercase()
        .strip_suffix(".exe")
        .unwrap_or(&name.to_lowercase())
        .ends_with("-en")
}

fn is_en() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
        .map(|s| has_en_suffix(&s))
        .unwrap_or(false)
}

fn t(en: &str, zh: &str) -> String {
    if is_en() {
        en.to_string()
    } else {
        zh.to_string()
    }
}

fn index_html() -> &'static str {
    if is_en() {
        INDEX_HTML_EN
    } else {
        INDEX_HTML
    }
}

fn app_js() -> &'static str {
    if is_en() {
        APP_JS_EN
    } else {
        APP_JS
    }
}

struct Ctx {
    port: u16,
    token: String,
    socket: PathBuf,
    profile: PathBuf,
    /// 常驻上游连接：会话 holder 绑定在连接上，必须复用同一条连接。
    rpc: Mutex<Client>,
}

pub(crate) fn main() {
    let args = Args::parse(std::env::args().skip(1));
    if args.has("--help") || args.positional.iter().any(|p| p == "-h") {
        println!("FeatherAgent WebUI {}", env!("CARGO_PKG_VERSION"));
        println!(
            "{}",
            t(
                "Usage: featheragent-webui-en [--root DIR] [--socket PATH] [--app]",
                "用法: featheragent-webui [--root DIR] [--socket PATH] [--app]"
            )
        );
        return;
    }
    if args.has("version") {
        println!("FeatherAgent WebUI {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("featheragent-webui"));
    let overrides = Overrides {
        install_root: args.flag("root").map(PathBuf::from),
        ..Default::default()
    };
    let paths = Paths::resolve(&exe, &overrides);
    let socket = args
        .flag("socket")
        .map(PathBuf::from)
        .unwrap_or_else(|| paths.socket());

    // 内核必须已在跑：先握手确认契约可用；这条连接后续承接所有浏览器 RPC，
    // 保证 session.create/resume 的 holder 与 chat.send 是同一条连接。
    let rpc_conn = match Client::connect(&socket) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", t(&format!("Cannot connect to kernel {socket:?}: {e}\nStart `featheragent serve` first"), &format!("无法连接内核 {socket:?}: {e}\n先运行 `featheragent serve`")));
            std::process::exit(2);
        }
    };
    if let Err(e) =
        rpc_conn
            .client
            .call_with_timeout("initialize", json!({}), Duration::from_secs(3))
    {
        eprintln!(
            "{}",
            t(
                &format!("Kernel not ready ({socket:?}): {e}"),
                &format!("内核未就绪（{socket:?}）: {e}")
            )
        );
        std::process::exit(2);
    }
    // 该连接只做 RPC；通知由 SSE 连接单独订阅。这里排空事件，避免无界通道增长。
    let rpc_events = rpc_conn.events;
    thread::spawn(move || while rpc_events.recv().is_ok() {});
    let rpc = Mutex::new(rpc_conn.client);

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let token = random_token(24);
    let profile = paths.runtime.join("webui-profile");
    std::fs::create_dir_all(&profile).ok();
    let ctx = Arc::new(Ctx {
        port,
        token,
        socket,
        profile,
        rpc,
    });

    let url = format!("http://127.0.0.1:{}/#token={}", ctx.port, ctx.token);
    println!(
        "{}",
        serde_json::to_string(&json!({
            "ok": true,
            "url": url,
            "port": ctx.port,
            "bind": "127.0.0.1",
            "token_in_fragment": true,
            "webui_profile": ctx.profile.display().to_string(),
            "app_mode": args.has("--app"),
        }))
        .unwrap()
    );

    if args.has("--app") {
        launch_browser(&url, &ctx.profile);
    } else {
        println!("{}", t("Open the URL above in a browser (the token is in the #fragment and is not sent in Referer/logs)", "在浏览器打开上面的 url（token 在 #fragment 中，不会进 Referer/日志）"));
    }

    if let Err(e) = serve(listener, ctx) {
        eprintln!(
            "{}",
            t(
                &format!("WebUI bridge exited: {e}"),
                &format!("WebUI 桥退出: {e}")
            )
        );
    }
}

fn serve(listener: TcpListener, ctx: Arc<Ctx>) -> std::io::Result<()> {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let ctx = ctx.clone();
        thread::spawn(move || {
            let _ = handle(stream, ctx);
        });
    }
    Ok(())
}

struct Req {
    method: String,
    path: String,
    query: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl Req {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(|s| s.as_str())
    }
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<Req> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let header_end = loop {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "eof",
            ));
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > 64 * 1024 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "header too large",
            ));
        }
        if let Some(p) = find_sub(&buf, b"\r\n\r\n") {
            break p + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target, String::new()),
    };
    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let len: usize = headers
        .get("content-length")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if len > 1024 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "body too large",
        ));
    }
    let mut body = buf[header_end..].to_vec();
    while body.len() < len {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(len);
    Ok(Req {
        method,
        path,
        query,
        headers,
        body,
    })
}

fn find_sub(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn query_get(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        if k == key {
            Some(urldecode(v))
        } else {
            None
        }
    })
}

fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn host_ok(req: &Req, port: u16) -> bool {
    let allowed = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    let host_ok = req
        .header("host")
        .map(|h| allowed.iter().any(|a| a.eq_ignore_ascii_case(h)))
        .unwrap_or(false);
    if !host_ok {
        return false;
    }
    match req.header("origin") {
        None => true,
        Some(o) => [
            format!("http://127.0.0.1:{port}"),
            format!("http://localhost:{port}"),
        ]
        .iter()
        .any(|a| a.eq_ignore_ascii_case(o)),
    }
}

fn handle(mut stream: TcpStream, ctx: Arc<Ctx>) -> std::io::Result<()> {
    let req = read_request(&mut stream)?;
    if !host_ok(&req, ctx.port) {
        return respond(
            &mut stream,
            403,
            "text/plain; charset=utf-8",
            b"forbidden host/origin",
        );
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") | ("GET", "/index.html") => {
            let html = index_html().replace("{{VERSION}}", env!("CARGO_PKG_VERSION"));
            respond(
                &mut stream,
                200,
                "text/html; charset=utf-8",
                html.as_bytes(),
            )
        }
        ("GET", "/app.js") => respond(
            &mut stream,
            200,
            "application/javascript; charset=utf-8",
            app_js().as_bytes(),
        ),
        ("GET", "/style.css") => respond(
            &mut stream,
            200,
            "text/css; charset=utf-8",
            STYLE_CSS.as_bytes(),
        ),
        ("GET", "/favicon.ico") => respond(&mut stream, 200, "image/x-icon", FAVICON_ICO),
        ("GET", "/logo.png") => respond(&mut stream, 200, "image/png", LOGO_PNG),
        ("GET", "/logo-tile.png") => respond(&mut stream, 200, "image/png", LOGO_TILE_PNG),
        ("GET", "/events") => {
            let Some(t) = query_get(&req.query, "token") else {
                return respond(&mut stream, 401, "text/plain", b"missing token");
            };
            if t != ctx.token {
                return respond(&mut stream, 401, "text/plain", b"bad token");
            }
            stream_sse(stream, ctx)
        }
        ("POST", "/rpc") => rpc(stream, req, ctx),
        ("GET", "/ws") => {
            let Some(t) = query_get(&req.query, "token") else {
                return respond(&mut stream, 401, "text/plain", b"missing token");
            };
            if t != ctx.token {
                return respond(&mut stream, 401, "text/plain", b"bad token");
            }
            ws_bridge(stream, req, ctx)
        }
        _ => respond(&mut stream, 404, "text/plain", b"not found"),
    }
}

fn respond(stream: &mut TcpStream, status: u16, ctype: &str, body: &[u8]) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        400 => "Bad Request",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

fn rpc(mut stream: TcpStream, req: Req, ctx: Arc<Ctx>) -> std::io::Result<()> {
    let auth = req.header("authorization").unwrap_or("");
    let token = auth.strip_prefix("Bearer ").unwrap_or("");
    if token != ctx.token {
        return respond(
            &mut stream,
            401,
            "application/json",
            br#"{"error":"bad token"}"#,
        );
    }
    let v: Value = match serde_json::from_slice(&req.body) {
        Ok(v) => v,
        Err(_) => {
            return respond(
                &mut stream,
                400,
                "application/json",
                br#"{"error":"bad json"}"#,
            )
        }
    };
    let id = v.get("id").cloned().unwrap_or(Value::Null);
    let Some(method) = v.get("method").and_then(|m| m.as_str()) else {
        return respond(
            &mut stream,
            400,
            "application/json",
            br#"{"error":"method required"}"#,
        );
    };
    let params = v.get("params").cloned().unwrap_or(json!({}));
    let reply = {
        let c = ctx.rpc.lock().unwrap_or_else(|e| e.into_inner());
        match c.call(method, params) {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": 500, "message": e}}),
        }
    };
    let body = serde_json::to_vec(&reply).unwrap_or_default();
    respond(&mut stream, 200, "application/json; charset=utf-8", &body)
}

fn stream_sse(mut stream: TcpStream, ctx: Arc<Ctx>) -> std::io::Result<()> {
    let conn = match Client::connect(&ctx.socket) {
        Ok(c) => c,
        Err(_) => return respond(&mut stream, 503, "text/plain", b"kernel unavailable"),
    };
    if conn.client.call("initialize", json!({})).is_err() {
        return respond(&mut stream, 503, "text/plain", b"initialize failed");
    }
    let _ = conn.client.call("event.subscribe", json!({"kinds": []}));
    stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: keep-alive\r\nX-Accel-Buffering: no\r\n\r\n",
    )?;
    stream.flush()?;
    loop {
        match conn.events.recv_timeout(Duration::from_secs(15)) {
            Ok(v) => {
                let line = format!(
                    "data: {}\n\n",
                    serde_json::to_string(&v).unwrap_or_default()
                );
                if stream.write_all(line.as_bytes()).is_err() {
                    break;
                }
                let _ = stream.flush();
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if stream.write_all(b": ping\n\n").is_err() {
                    break;
                }
                let _ = stream.flush();
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}

// ---------- WebSocket 桥（§7.2：浏览器 ↔ WebUI 桥 = HTTP + WS） ----------

fn ws_bridge(mut stream: TcpStream, req: Req, ctx: Arc<Ctx>) -> std::io::Result<()> {
    let key = match req.header("sec-websocket-key") {
        Some(k) => k.to_string(),
        None => return respond(&mut stream, 400, "text/plain", b"missing sec-websocket-key"),
    };
    if !req
        .header("upgrade")
        .map(|u| u.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false)
    {
        return respond(&mut stream, 400, "text/plain", b"not a websocket upgrade");
    }
    let accept = base64(&sha1(
        format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").as_bytes(),
    ));
    let head = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    stream.write_all(head.as_bytes())?;
    stream.flush()?;

    // 每个浏览器页面一条上游 UDS 连接：该页面创建的会话 holder 就是它自己
    let conn = match Client::connect(&ctx.socket) {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let writer_stream = stream.try_clone()?;
    let writer = thread::spawn(move || {
        let mut s = writer_stream;
        for msg in rx {
            if write_ws_text(&mut s, &msg).is_err() {
                break;
            }
        }
        let _ = s.shutdown(std::net::Shutdown::Both);
    });
    let events = conn.events;
    let etx = tx.clone();
    let ev_thread = thread::spawn(move || loop {
        match events.recv_timeout(Duration::from_secs(15)) {
            Ok(v) => {
                let line = serde_json::to_string(&v).unwrap_or_default();
                if etx.send(line).is_err() {
                    break;
                }
            }
            // 空闲超时不能退出：页面可能长时间不操作，之后仍要接收新回合事件。
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    });

    // 请求循环（同连接上的 RPC 复用同一条 UDS，holder 语义正确）
    loop {
        let msg = match read_ws_text(&mut stream) {
            Ok(Some(t)) => t,
            Ok(None) => break,
            Err(_) => break,
        };
        let v: Value = match serde_json::from_str(&msg) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let id = v.get("id").cloned();
        let Some(method) = v.get("method").and_then(|m| m.as_str()) else {
            continue;
        };
        let params = v.get("params").cloned().unwrap_or(json!({}));
        let reply = match conn.client.call(method, params) {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": 500, "message": e}}),
        };
        if tx.send(reply.to_string()).is_err() {
            break;
        }
    }
    drop(tx);
    let _ = writer.join();
    ev_thread.join().ok();
    Ok(())
}

fn read_ws_text(stream: &mut TcpStream) -> std::io::Result<Option<String>> {
    stream.set_read_timeout(None)?;
    loop {
        let mut h = [0u8; 2];
        stream.read_exact(&mut h)?;
        let opcode = h[0] & 0x0f;
        let fin = h[0] & 0x80 != 0;
        let masked = h[1] & 0x80 != 0;
        let mut len = (h[1] & 0x7f) as u64;
        if len == 126 {
            let mut b = [0u8; 2];
            stream.read_exact(&mut b)?;
            len = u16::from_be_bytes(b) as u64;
        } else if len == 127 {
            let mut b = [0u8; 8];
            stream.read_exact(&mut b)?;
            len = u64::from_be_bytes(b);
        }
        if len > 1024 * 1024 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "ws frame too large",
            ));
        }
        let mut mask = [0u8; 4];
        if masked {
            stream.read_exact(&mut mask)?;
        }
        let mut payload = vec![0u8; len as usize];
        stream.read_exact(&mut payload)?;
        if masked {
            for (i, b) in payload.iter_mut().enumerate() {
                *b ^= mask[i % 4];
            }
        }
        match opcode {
            0x1 | 0x2 => {
                if !fin {
                    let mut all = payload;
                    loop {
                        let mut h2 = [0u8; 2];
                        stream.read_exact(&mut h2)?;
                        let op2 = h2[0] & 0x0f;
                        let fin2 = h2[0] & 0x80 != 0;
                        let mut l2 = (h2[1] & 0x7f) as u64;
                        if l2 == 126 {
                            let mut b = [0u8; 2];
                            stream.read_exact(&mut b)?;
                            l2 = u16::from_be_bytes(b) as u64;
                        } else if l2 == 127 {
                            let mut b = [0u8; 8];
                            stream.read_exact(&mut b)?;
                            l2 = u64::from_be_bytes(b);
                        }
                        if l2 > 1024 * 1024 || op2 != 0x0 {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "bad fragment",
                            ));
                        }
                        let masked2 = h2[1] & 0x80 != 0;
                        if masked2 {
                            let mut m2 = [0u8; 4];
                            stream.read_exact(&mut m2)?;
                            let mut p2 = vec![0u8; l2 as usize];
                            stream.read_exact(&mut p2)?;
                            for (i, b) in p2.iter_mut().enumerate() {
                                *b ^= m2[i % 4];
                            }
                            all.extend_from_slice(&p2);
                        } else {
                            let mut p2 = vec![0u8; l2 as usize];
                            stream.read_exact(&mut p2)?;
                            all.extend_from_slice(&p2);
                        }
                        if fin2 {
                            break;
                        }
                    }
                    return Ok(Some(String::from_utf8_lossy(&all).to_string()));
                }
                return Ok(Some(String::from_utf8_lossy(&payload).to_string()));
            }
            0x8 => return Ok(None),
            0x9 => {
                write_ws_frame(stream, 0xA, &payload)?;
            }
            0xA => {}
            _ => {}
        }
    }
}

fn write_ws_text(stream: &mut TcpStream, s: &str) -> std::io::Result<()> {
    write_ws_frame(stream, 0x1, s.as_bytes())
}

fn write_ws_frame(stream: &mut TcpStream, opcode: u8, payload: &[u8]) -> std::io::Result<()> {
    let mut head = Vec::with_capacity(10);
    head.push(0x80 | opcode);
    let n = payload.len();
    if n <= 125 {
        head.push(n as u8);
    } else if n <= 65535 {
        head.push(126);
        head.extend_from_slice(&(n as u16).to_be_bytes());
    } else {
        head.push(127);
        head.extend_from_slice(&(n as u64).to_be_bytes());
    }
    stream.write_all(&head)?;
    stream.write_all(payload)?;
    stream.flush()
}

fn launch_browser(url: &str, profile: &PathBuf) {
    #[cfg(unix)]
    let candidates: Vec<(String, bool)> = vec![
        ("chromium".into(), true),
        ("chromium-browser".into(), true),
        ("google-chrome".into(), true),
        ("google-chrome-stable".into(), true),
        ("microsoft-edge".into(), true),
        ("firefox".into(), false),
    ];
    #[cfg(windows)]
    let candidates: Vec<(String, bool)> = vec![
        (
            r"C:\Program Files\Google\Chrome\Application\chrome.exe".into(),
            true,
        ),
        (
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe".into(),
            true,
        ),
        (
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe".into(),
            true,
        ),
        (
            r"C:\Program Files\Microsoft\Edge\Application\msedge.exe".into(),
            true,
        ),
        ("chrome.exe".into(), true),
        ("msedge.exe".into(), true),
        ("chromium.exe".into(), true),
        ("firefox.exe".into(), false),
    ];
    for (bin, chromium_like) in candidates {
        #[cfg(unix)]
        let ok = Command::new("sh")
            .arg("-c")
            .arg(format!("command -v {bin} >/dev/null 2>&1"))
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        #[cfg(windows)]
        let ok = std::path::Path::new(&bin).is_file()
            || Command::new("where")
                .arg(&bin)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
        if !ok {
            continue;
        }
        let mut cmd = Command::new(&bin);
        if chromium_like {
            cmd.arg(format!("--app={url}"))
                .arg(format!("--user-data-dir={}", profile.display()))
                .arg("--no-first-run")
                .arg("--no-default-browser-check")
                .arg("--disable-features=Translate")
                .arg("--window-size=1280,800");
        } else {
            // Firefox 无 app 模式；独立 profile 目录同样隔离
            cmd.arg("--new-window")
                .arg(url)
                .arg("--profile")
                .arg(profile);
        }
        match cmd.spawn() {
            Ok(child) => {
                println!(
                    "{}",
                    t(
                        &format!(
                            "WebUI started ({}, pid {}, profile {})",
                            bin,
                            child.id(),
                            profile.display()
                        ),
                        &format!(
                            "已启动 WebUI（{}，pid {}，profile {}）",
                            bin,
                            child.id(),
                            profile.display()
                        )
                    )
                );
                return;
            }
            Err(e) => eprintln!(
                "{}",
                t(
                    &format!("Failed to start {bin}: {e}"),
                    &format!("启动 {bin} 失败: {e}")
                )
            ),
        }
    }
    eprintln!(
        "{}",
        t(
            "No Chromium-based browser found; open the URL manually (skipped in --app mode)",
            "未找到 Chromium 系浏览器；请手动打开 URL（--app 模式跳过）"
        )
    );
}

// 供 `cargo test` 里做冒烟检查的最小自测
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    #[test]
    fn urldecode_basic() {
        assert_eq!(urldecode("a%20b+c"), "a b c");
    }

    #[test]
    fn english_suffix_selects_english_assets() {
        assert!(has_en_suffix("featheragent-webui-en"));
        assert!(has_en_suffix("featheragent-webui-en.exe"));
        assert!(!has_en_suffix("featheragent-webui"));
        assert!(INDEX_HTML_EN.contains("Settings"));
        assert!(!INDEX_HTML_EN.contains("设置"));
        assert!(APP_JS_EN.contains("RPC timeout"));
        assert!(!APP_JS_EN.contains("RPC 超时"));
    }

    #[test]
    fn static_assets_nonempty() {
        assert!(INDEX_HTML.contains("FeatherAgent"));
        assert!(INDEX_HTML.contains("settings-page"));
        assert!(INDEX_HTML.contains("cfg-temperature"));
        assert!(INDEX_HTML.contains("cfg-model-select"));
        assert!(!INDEX_HTML.contains("cfg-model-list"));
        assert!(INDEX_HTML.contains("settings-nav"));
        assert!(INDEX_HTML.contains("cfg-workspace-root"));
        assert!(INDEX_HTML.contains("plugin-list"));
        assert!(INDEX_HTML.contains("plugin-install-path"));
        assert!(APP_JS.contains("EventSource"));
        assert!(APP_JS.contains("provider.models"));
        assert!(APP_JS.contains("RPC 超时"));
        assert!(APP_JS.contains("config.set"));
        assert!(APP_JS.contains("renderModelList"));
        assert!(APP_JS.contains("cfg-model-select"));
        assert!(APP_JS.contains("renderPluginNav"));
        assert!(APP_JS.contains("installPlugin"));
        assert!(APP_JS.contains("showSettingsView"));
    }

    fn masked_frame(opcode: u8, fin: bool, text: &str) -> Vec<u8> {
        let payload = text.as_bytes();
        assert!(payload.len() <= 125);
        let mut out = Vec::new();
        out.push(if fin { 0x80 } else { 0 } | opcode);
        out.push(0x80 | payload.len() as u8);
        let mask = [0x12u8, 0x34, 0x56, 0x78];
        out.extend_from_slice(&mask);
        for (i, b) in payload.iter().enumerate() {
            out.push(*b ^ mask[i % 4]);
        }
        out
    }

    #[test]
    fn fragmented_masked_text_is_reassembled() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            s.write_all(&masked_frame(0x1, false, "hello ")).unwrap();
            s.write_all(&masked_frame(0x0, true, "world")).unwrap();
        });
        let mut client = TcpStream::connect(addr).unwrap();
        let got = read_ws_text(&mut client).unwrap();
        assert_eq!(got.as_deref(), Some("hello world"));
        server.join().unwrap();
    }
}
