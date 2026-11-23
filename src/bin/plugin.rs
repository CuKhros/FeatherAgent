//! `feather-plugin`：出厂插件多合一二进制（provider / tool）。
//!
//! 用法：`feather-plugin <role>`，role ∈ {mock, ollama, tool}。
//! 传输：stdio + 长度前缀 msgpack 帧。所有输出只走 stdout 的帧，日志走 stderr。

use std::collections::{BTreeMap, HashSet};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde_json::{json, Value};

use featheragent::net;
use featheragent::proto::types::PluginManifest;
use featheragent::proto::{read_frame, write_frame, Codec};

struct Io {
    out: Mutex<std::io::Stdout>,
    codec: Codec,
    cancelled: Mutex<HashSet<u64>>,
}

impl Io {
    fn send(&self, v: Value) {
        let mut o = self.out.lock();
        let _ = write_frame(&mut *o, &v, self.codec);
    }

    fn respond(&self, id: u64, result: Value) {
        self.send(json!({"jsonrpc": "2.0", "id": id, "result": result}));
    }

    fn fail(&self, id: u64, code: i64, msg: &str) {
        self.send(json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": msg}}));
    }

    fn emit(&self, req: u64, mut extra: Value) {
        if let Some(o) = extra.as_object_mut() {
            o.insert("req".into(), json!(req));
        }
        self.send(json!({"jsonrpc": "2.0", "method": "plugin.event", "params": extra}));
    }

    fn is_cancelled(&self, req: u64) -> bool {
        self.cancelled.lock().contains(&req)
    }
}

fn main() {
    let role = std::env::args().nth(1).unwrap_or_else(|| "mock".into());
    let codec = if std::env::var("FEATHER_CODEC").ok().as_deref() == Some("json") {
        Codec::Json
    } else {
        Codec::MsgPack
    };
    let io = Arc::new(Io {
        out: Mutex::new(std::io::stdout()),
        codec,
        cancelled: Mutex::new(HashSet::new()),
    });

    let stdin = std::io::stdin();
    let mut lock = stdin.lock();
    loop {
        match read_frame(&mut lock, codec) {
            Ok(Some(v)) => {
                let method = v
                    .get("method")
                    .and_then(|m| m.as_str())
                    .unwrap_or("")
                    .to_string();
                let id = v.get("id").and_then(|x| x.as_u64()).unwrap_or(0);
                let params = v.get("params").cloned().unwrap_or(Value::Null);
                if method == "$/cancel" {
                    if let Some(req) = params.get("req").and_then(|x| x.as_u64()) {
                        io.cancelled.lock().insert(req);
                    }
                    continue;
                }
                let io2 = io.clone();
                let role2 = role.clone();
                thread::spawn(move || {
                    if let Some((code, msg)) = handle(&role2, &method, &params, id, &io2) {
                        io2.fail(id, code, &msg);
                    }
                });
            }
            Ok(None) => break,
            Err(e) => {
                eprintln!("read_frame error: {e}");
                break;
            }
        }
    }
}

fn manifest(role: &str) -> PluginManifest {
    let (name, caps, perms, cat) = match role {
        "mock" => (
            "provider-mock",
            vec!["chat", "stream", "tools", "embed"],
            vec![],
            "provider",
        ),
        "ollama" => (
            "provider-ollama",
            vec!["chat", "stream", "tools", "embed", "fim"],
            vec!["net:local"],
            "provider",
        ),
        "tool" => (
            "tool",
            vec!["tool.fs.read", "tool.fs.write", "tool.shell", "tool.ping"],
            vec!["fs:read", "fs:write", "exec:shell"],
            "tool",
        ),
        _ => (role, vec![], vec![], "tool"),
    };
    PluginManifest {
        name: name.into(),
        version: featheragent::VERSION.into(),
        api: ">=1.0 <2.0".into(),
        capabilities: caps.into_iter().map(|s| s.to_string()).collect(),
        permissions: perms.into_iter().map(|s| s.to_string()).collect(),
        category: cat.into(),
    }
}

fn handle(
    role: &str,
    method: &str,
    params: &Value,
    id: u64,
    io: &Arc<Io>,
) -> Option<(i64, String)> {
    if method == "initialize" {
        io.respond(id, serde_json::to_value(manifest(role)).unwrap());
        return None;
    }
    let res = match role {
        "mock" => provider_mock(method, params, io, id),
        "ollama" => provider_ollama(method, params, io, id),
        "tool" => tools(method, params),
        _ => Err(format!("unknown role {role}")),
    };
    match res {
        Ok(v) => {
            io.respond(id, v);
            None
        }
        Err(e) => Some((-32000, e)),
    }
}

// ---------------- mock provider（确定性，供 e2e / 门禁） --------------

fn provider_mock(method: &str, params: &Value, io: &Arc<Io>, id: u64) -> Result<Value, String> {
    let req = params.get("req").and_then(|x| x.as_u64()).unwrap_or(id);
    match method {
        "probe" => Ok(json!({
            "name": "mock",
            "capabilities": ["chat", "stream", "tools", "embed"],
            "model": params.get("model").cloned().unwrap_or(json!("mock-7b")),
            "models": [params.get("model").cloned().unwrap_or(json!("mock-7b"))],
            "context_window": 8192,
        })),
        "embed" => {
            let text = params.get("text").and_then(|x| x.as_str()).unwrap_or("");
            let mut vec = Vec::with_capacity(32);
            let digest = *blake3::hash(text.as_bytes()).as_bytes();
            for chunk in digest.chunks(2) {
                vec.push(u16::from_le_bytes([chunk[0], chunk[1]]) as f32 / 65535.0);
            }
            Ok(json!({"vector": vec, "dim": vec.len(), "model": "mock-embed"}))
        }
        "chat" => ok_chat_stream(params, io, req, |last_user, last_is_tool| {
            if last_is_tool {
                return Ok(("已完成。".into(), 0));
            }
            if last_user.contains("CRASH_ONCE") {
                // 第一次崩溃，重启后再问就正常回答（验证 §4.5 的重试 1 次）
                let data = std::env::var("FEATHER_DATA").unwrap_or_else(|_| "/tmp".into());
                let marker = std::path::Path::new(&data).join(".crash_once");
                if marker.exists() {
                    return Ok((
                        format!(
                            "restarted: {}",
                            last_user.chars().take(20).collect::<String>()
                        ),
                        0,
                    ));
                }
                let _ = std::fs::create_dir_all(&data);
                let _ = std::fs::write(&marker, b"1");
                std::process::exit(1);
            }
            if last_user.contains("CRASH") {
                std::process::exit(1);
            }
            if let Some(rest) = last_user.split_once("TOOL:").map(|(_, r)| r) {
                return Ok((rest.trim().to_string(), 0));
            }
            if let Some(rest) = last_user.split_once("READ:").map(|(_, r)| r) {
                return Ok((
                    json!({"name":"tool.fs.read","arguments":{"path": rest.trim()}}).to_string(),
                    0,
                ));
            }
            if last_user.contains("SPIN") {
                return Ok(("SPIN".into(), 100));
            }
            let short: String = last_user.chars().take(60).collect();
            Ok((format!("echo: {short}"), 0))
        }),
        _ => Err(format!("mock: 未知方法 {method}")),
    }
}

fn ok_chat_stream<F>(params: &Value, io: &Arc<Io>, req: u64, f: F) -> Result<Value, String>
where
    F: FnOnce(&str, bool) -> Result<(String, u64), String>,
{
    let messages = params
        .get("messages")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();
    let last_user = messages
        .iter()
        .rev()
        .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
        .and_then(|m| {
            m.get("content")
                .and_then(|c| c.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_default();
    let last_is_tool = messages
        .last()
        .and_then(|m| m.get("role").and_then(|r| r.as_str()))
        .map(|r| r == "tool")
        .unwrap_or(false);
    let (text, delay_ms) = f(&last_user, last_is_tool)?;

    if text == "SPIN" {
        let mut elapsed = 0u64;
        while elapsed < 30_000 {
            if io.is_cancelled(req) {
                io.emit(req, json!({"kind": "done", "finish_reason": "cancelled"}));
                return Ok(json!({"text": "", "finish_reason": "cancelled"}));
            }
            io.emit(req, json!({"kind": "delta", "text": "."}));
            thread::sleep(Duration::from_millis(100));
            elapsed += 100;
        }
    }

    if delay_ms > 0 && !text.is_empty() {
        thread::sleep(Duration::from_millis(delay_ms));
    }
    if params
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(true)
    {
        // 按字符切块：按字节切会把中文/emoji 的多字节序列切断成乱码
        let chars: Vec<char> = text.chars().collect();
        for chunk in chars.chunks(16) {
            if io.is_cancelled(req) {
                io.emit(req, json!({"kind": "done", "finish_reason": "cancelled"}));
                return Ok(json!({"text": text, "finish_reason": "cancelled"}));
            }
            let piece: String = chunk.iter().collect();
            io.emit(req, json!({"kind": "delta", "text": piece}));
            thread::sleep(Duration::from_millis(2));
        }
    }
    io.emit(req, json!({"kind": "done", "finish_reason": "stop"}));
    Ok(json!({"text": text, "finish_reason": "stop"}))
}

// ---------------- ollama / openai 兼容 provider ----------------

fn provider_ollama(method: &str, params: &Value, io: &Arc<Io>, id: u64) -> Result<Value, String> {
    let base = params
        .get("base_url")
        .and_then(|x| x.as_str())
        .unwrap_or("http://127.0.0.1:11434")
        .trim_end_matches('/')
        .to_string();
    let model = params
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("qwen2.5:7b");
    let req = params.get("req").and_then(|x| x.as_u64()).unwrap_or(id);
    match method {
        "probe" => {
            let openai = base.contains("/v1");
            let url = if openai {
                format!("{base}/models")
            } else {
                format!("{base}/api/tags")
            };
            let r = net::get_json_timeout(&url, std::time::Duration::from_secs(5))?;
            let v: Value = r.json()?;
            let mut models = Vec::new();
            if openai {
                for m in v
                    .get("data")
                    .and_then(|x| x.as_array())
                    .into_iter()
                    .flatten()
                {
                    if let Some(id) = m.get("id").and_then(|x| x.as_str()) {
                        models.push(Value::String(id.to_string()));
                    }
                }
            } else {
                for m in v
                    .get("models")
                    .and_then(|x| x.as_array())
                    .into_iter()
                    .flatten()
                {
                    if let Some(name) = m.get("name").and_then(|x| x.as_str()) {
                        models.push(Value::String(name.to_string()));
                    }
                }
            }
            Ok(json!({
                "name": "ollama",
                "capabilities": ["chat", "stream", "tools", "embed", "fim"],
                "base_url": base,
                "models": models,
            }))
        }
        "embed" => {
            let text = params.get("text").and_then(|x| x.as_str()).unwrap_or("");
            let r = net::post_json(
                &format!("{base}/api/embed"),
                &json!({"model": model, "input": text}),
            )?;
            let v = r.json()?;
            let arr = v
                .get("embeddings")
                .and_then(|e| e.get(0))
                .cloned()
                .unwrap_or(Value::Null);
            Ok(json!({"vector": arr, "model": model}))
        }
        "chat" => {
            let messages = params.get("messages").cloned().unwrap_or(json!([]));
            let openai = base.contains("/v1");
            let mut body = json!({"model": model, "messages": messages, "stream": true});
            if let Some(t) = params.get("tools").cloned() {
                if t.as_array().map(|a| !a.is_empty()).unwrap_or(false) {
                    body["tools"] = t;
                }
            }
            // 空响应重试会带 temperature：OpenAI 放顶层，Ollama 放 options。
            if let Some(t) = params.get("temperature").cloned() {
                if openai {
                    body["temperature"] = t;
                } else {
                    body["options"] = json!({"temperature": t});
                }
            }
            let mut acc = String::new();
            let mut native_calls: Vec<Value> = Vec::new();
            let mut openai_calls: BTreeMap<u64, (String, String)> = BTreeMap::new();
            let mut upstream_finish = "stop".to_string();
            let url = if openai {
                format!("{base}/chat/completions")
            } else {
                format!("{base}/api/chat")
            };
            net::stream_post_lines(&url, &body, |line| {
                if io.is_cancelled(req) {
                    return false;
                }
                let line = line.trim();
                if line.is_empty() {
                    return true;
                }
                let payload = if let Some(rest) = line.strip_prefix("data:") {
                    rest.trim()
                } else {
                    line
                };
                if payload == "[DONE]" {
                    return false;
                }
                let Ok(v) = serde_json::from_str::<Value>(payload) else {
                    return true;
                };
                if let Some(r) = v.get("done_reason").and_then(|x| x.as_str()) {
                    upstream_finish = r.to_string();
                }
                if let Some(r) = v
                    .pointer("/choices/0/finish_reason")
                    .and_then(|x| x.as_str())
                {
                    upstream_finish = r.to_string();
                }
                let delta = if openai {
                    v.pointer("/choices/0/delta/content")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string()
                } else {
                    v.pointer("/message/content")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string()
                };
                if !delta.is_empty() {
                    acc.push_str(&delta);
                    io.emit(req, json!({"kind": "delta", "text": delta}));
                }
                if openai {
                    if let Some(calls) = v
                        .pointer("/choices/0/delta/tool_calls")
                        .and_then(|x| x.as_array())
                    {
                        for c in calls {
                            let idx = c.get("index").and_then(|x| x.as_u64()).unwrap_or(0);
                            let e = openai_calls.entry(idx).or_default();
                            if let Some(name) = c.pointer("/function/name").and_then(|x| x.as_str())
                            {
                                if e.0.is_empty() {
                                    e.0.push_str(name);
                                } else if !e.0.ends_with(name) {
                                    e.0.push_str(name);
                                }
                            }
                            if let Some(args) =
                                c.pointer("/function/arguments").and_then(|x| x.as_str())
                            {
                                e.1.push_str(args);
                            }
                        }
                    }
                    if let Some(calls) = v
                        .pointer("/choices/0/message/tool_calls")
                        .and_then(|x| x.as_array())
                    {
                        for c in calls {
                            if let Some(name) = c.pointer("/function/name").and_then(|x| x.as_str())
                            {
                                let args = c
                                    .pointer("/function/arguments")
                                    .cloned()
                                    .unwrap_or_else(|| json!({}));
                                native_calls.push(json!({"name": name, "arguments": args}));
                            }
                        }
                    }
                } else if let Some(calls) =
                    v.pointer("/message/tool_calls").and_then(|x| x.as_array())
                {
                    for c in calls {
                        if let Some(name) = c.pointer("/function/name").and_then(|x| x.as_str()) {
                            let args = c
                                .pointer("/function/arguments")
                                .cloned()
                                .unwrap_or_else(|| json!({}));
                            native_calls.push(json!({"name": name, "arguments": args}));
                        }
                    }
                }
                true
            })?;
            for (_, (name, args)) in openai_calls {
                if name.is_empty() {
                    continue;
                }
                let args = if args.trim().is_empty() {
                    json!({})
                } else {
                    serde_json::from_str(&args).unwrap_or_else(|_| json!({"raw": args}))
                };
                native_calls.push(json!({"name": name, "arguments": args}));
            }
            let calls = featheragent::context::parse::from_value(&Value::Array(native_calls));
            if acc.trim().is_empty() && calls.is_empty() {
                let msg = "Ollama 返回空响应（content 与 tool_calls 均为空）；已拒绝以 status=ok 静默通过。请检查 base_url/代理/模型是否就绪";
                io.emit(req, json!({"kind": "error", "message": msg}));
                return Err(msg.to_string());
            }
            let finish = if calls.is_empty() {
                if upstream_finish.is_empty() {
                    "stop".into()
                } else {
                    upstream_finish
                }
            } else {
                "tool_calls".into()
            };
            let calls_json = Value::Array(
                calls
                    .iter()
                    .map(|c| json!({"name": c.name, "arguments": c.arguments}))
                    .collect(),
            );
            io.emit(
                req,
                json!({"kind": "done", "finish_reason": finish, "tool_calls": calls_json}),
            );
            Ok(json!({"text": acc, "finish_reason": finish, "tool_calls": calls_json}))
        }
        "fim" => {
            let prompt = params.get("prompt").and_then(|x| x.as_str()).unwrap_or("");
            let r = net::post_json(
                &format!("{base}/api/generate"),
                &json!({"model": model, "prompt": prompt, "stream": false, "raw": true}),
            )?;
            Ok(json!({"text": r.json()?.get("response").cloned().unwrap_or(Value::Null)}))
        }
        _ => Err(format!("ollama: 未知方法 {method}")),
    }
}

// ---------------- tools ----------------

fn tools(method: &str, params: &Value) -> Result<Value, String> {
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| params.clone());
    let workspace = params
        .get("workspace")
        .and_then(|x| x.as_str())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("featheragent-ws"));
    std::fs::create_dir_all(&workspace).map_err(|e| e.to_string())?;
    match method {
        "tool.ping" => Ok(json!({"pong": true})),
        "tool.fs.read" => {
            let path = args
                .get("path")
                .and_then(|x| x.as_str())
                .ok_or("缺少 path")?;
            let (base, rel) = tool_base(&workspace, path);
            let p = safe_path(&base, &rel)?;
            let meta = std::fs::metadata(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            if meta.len() > 4 * 1024 * 1024 {
                return Err("文件超过 4 MB".into());
            }
            let content = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
            Ok(json!({"content": content, "bytes": meta.len(),
                "path": p.strip_prefix(&base).unwrap_or(&p).display().to_string()}))
        }
        "tool.fs.write" => {
            let path = args
                .get("path")
                .and_then(|x| x.as_str())
                .ok_or("缺少 path")?;
            let content = args
                .get("content")
                .and_then(|x| x.as_str())
                .ok_or("缺少 content")?;
            let (base, rel) = tool_base(&workspace, path);
            let p = safe_path(&base, &rel)?;
            if let Some(dir) = p.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(&p, content).map_err(|e| e.to_string())?;
            Ok(
                json!({"bytes": content.len(), "path": p.strip_prefix(&base).unwrap_or(&p).display().to_string()}),
            )
        }
        "tool.shell" => {
            let cmd = args.get("cmd").and_then(|x| x.as_str()).ok_or("缺少 cmd")?;
            #[cfg(unix)]
            let mut c = {
                let mut c = Command::new("/bin/sh");
                c.arg("-c").arg(cmd);
                c
            };
            #[cfg(windows)]
            let mut c = {
                // Windows 无 /bin/sh：用 cmd.exe；超时用 taskkill /T 杀进程树
                let mut c = Command::new("cmd.exe");
                c.arg("/C").arg(cmd);
                c
            };
            c.current_dir(&workspace)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            for v in [
                "RIPGREP_CONFIG_PATH",
                "XDG_CONFIG_HOME",
                "XDG_DATA_HOME",
                "HOME",
                "TMPDIR",
            ] {
                c.env_remove(v);
            }
            c.env("HOME", workspace.join(".featheagent").join(".home"));
            // 让 shell 成为新进程组组长：超时杀 -pid，连带杀掉 sleep 等子孙进程。
            // 只 kill shell 会让子进程继续持有 stdout 管道，wait_with_output 会挂住。
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                unsafe {
                    c.pre_exec(|| {
                        if libc::setsid() == -1 {
                            return Err(std::io::Error::last_os_error());
                        }
                        Ok(())
                    });
                }
            }
            let mut child = c.spawn().map_err(|e| e.to_string())?;
            let start = Instant::now();
            let mut timed_out = false;
            loop {
                if let Ok(Some(_)) = child.try_wait() {
                    break;
                }
                if start.elapsed() > Duration::from_secs(15) {
                    let pid = child.id();
                    #[cfg(unix)]
                    unsafe {
                        libc::kill(-(pid as i32), libc::SIGKILL)
                    };
                    #[cfg(windows)]
                    {
                        // 杀整棵进程树，避免 cmd 的子进程继续持有 stdout 管道
                        let _ = Command::new("taskkill")
                            .args(["/F", "/T", "/PID", &pid.to_string()])
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .status();
                    }
                    let _ = child.kill();
                    timed_out = true;
                    break;
                }
                thread::sleep(Duration::from_millis(20));
            }
            let out = child.wait_with_output().map_err(|e| e.to_string())?;
            Ok(json!({
                "code": out.status.code().unwrap_or(-1),
                "stdout": String::from_utf8_lossy(&out.stdout).chars().take(20_000).collect::<String>(),
                "stderr": String::from_utf8_lossy(&out.stderr).chars().take(8_000).collect::<String>(),
                "timed_out": timed_out,
            }))
        }
        _ => Err(format!("tool: 未知方法 {method}")),
    }
}

/// 路径守卫：只允许当前项目工作区内的文件（越界直接拒绝）。
/// `workspace` 参数现在就是项目根（不是 `<workspace_root>/<session>`）。
/// `workspace/xxx` 与普通 `xxx` 都解析到同一项目根，避免模型把容器根写进路径。
fn tool_base(workspace: &Path, user: &str) -> (PathBuf, String) {
    let mut norm = user.replace('\\', "/");
    while let Some(rest) = norm.strip_prefix("./") {
        norm = rest.to_string();
    }
    if let Some(rest) = norm.strip_prefix('/') {
        if rest == "workspace" || rest.starts_with("workspace/") {
            norm = rest.to_string();
        }
    }
    if norm == "workspace" {
        (workspace.to_path_buf(), ".".to_string())
    } else if let Some(rest) = norm.strip_prefix("workspace/") {
        (workspace.to_path_buf(), rest.to_string())
    } else {
        (workspace.to_path_buf(), norm)
    }
}

fn safe_path(workspace: &Path, user: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(workspace).map_err(|e| e.to_string())?;
    let base = std::fs::canonicalize(workspace).map_err(|e| e.to_string())?;
    let raw = if Path::new(user).is_absolute() {
        PathBuf::from(user)
    } else {
        base.join(user)
    };
    let candidate = if raw.exists() {
        std::fs::canonicalize(&raw).map_err(|e| e.to_string())?
    } else {
        let parent = raw.parent().unwrap_or(&base);
        let name = raw.file_name().ok_or("非法文件名")?.to_owned();
        let p = std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
        p.join(name)
    };
    if !candidate.starts_with(&base) {
        return Err(format!("路径越界：{}", candidate.display()));
    }
    Ok(candidate)
}

#[allow(dead_code)]
fn drain<R: BufRead>(r: &mut R) {
    let mut s = String::new();
    let _ = std::io::Read::read_to_string(r, &mut s);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_prefix_resolves_to_project_root() {
        let project = Path::new("/tmp/fa-root/workspace/proj");
        for p in [
            "workspace/a.txt",
            "/workspace/a.txt",
            "\\workspace\\a.txt",
            "./workspace/a.txt",
        ] {
            let (base, rel) = tool_base(project, p);
            assert_eq!(base, project, "base for {p}");
            assert_eq!(rel, "a.txt", "rel for {p}");
        }
        let (base, rel) = tool_base(project, "/etc/passwd");
        assert_eq!(base, project);
        assert_eq!(rel, "/etc/passwd");
    }

    #[test]
    fn safe_path_rejects_parent_escape() {
        let root = temp_root("escape");
        let ws = root.join("workspace").join("proj");
        std::fs::create_dir_all(&ws).unwrap();
        assert!(safe_path(&ws, "ok.txt").is_ok());
        assert!(safe_path(&ws, "../outside.txt").is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    fn temp_root(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("fa-plugin-{}-{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
