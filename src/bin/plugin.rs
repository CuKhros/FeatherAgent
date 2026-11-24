//! `feather-plugin`：出厂插件多合一二进制（provider / tool / loop）。
//!
//! 用法：`feather-plugin <role>`，role ∈ {mock, ollama, llamacpp, ninfer,
//! lmstudio, vllm, openai, tool, search, review-gate}。
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
        "llamacpp" => (
            "provider-llamacpp",
            vec!["chat", "stream", "tools", "embed"],
            vec!["net:local"],
            "provider",
        ),
        "ninfer" => (
            "provider-ninfer",
            vec!["chat", "stream", "tools", "embed"],
            vec!["net:local"],
            "provider",
        ),
        "lmstudio" => (
            "provider-lmstudio",
            vec!["chat", "stream", "tools", "embed"],
            vec!["net:local"],
            "provider",
        ),
        "vllm" => (
            "provider-vllm",
            vec!["chat", "stream", "tools", "embed"],
            vec!["net:local"],
            "provider",
        ),
        "openai" => (
            "provider-openai",
            vec!["chat", "stream", "tools", "embed"],
            vec!["net:local"],
            "provider",
        ),
        "review-gate" => (
            "review-gate",
            vec!["loop.review-gate", "review.guide", "review.judge"],
            vec![],
            "loop",
        ),
        "tool" => (
            "tool",
            vec!["tool.fs.read", "tool.fs.write", "tool.shell", "tool.ping"],
            vec!["fs:read", "fs:write", "exec:shell"],
            "tool",
        ),
        "search" => (
            "search",
            vec!["search", "search.fetch", "search.news", "search.suggest"],
            vec!["net:public"],
            "search",
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
        "ollama" => provider_http("ollama", method, params, io, id),
        "llamacpp" => provider_http("llamacpp", method, params, io, id),
        "ninfer" => provider_http("ninfer", method, params, io, id),
        "lmstudio" => provider_http("lmstudio", method, params, io, id),
        "vllm" => provider_http("vllm", method, params, io, id),
        "openai" => provider_http("openai", method, params, io, id),
        "review-gate" => review_gate_plugin(method, params),
        "tool" => tools(method, params),
        "search" => search_plugin(method, params),
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

fn search_plugin(method: &str, params: &Value) -> Result<Value, String> {
    match method {
        "probe" | "status" => Ok(featheragent::local_search::status()),
        "search" => featheragent::local_search::search(params),
        "news" => featheragent::local_search::news(params),
        "fetch" => featheragent::local_search::fetch(params),
        "suggest" => featheragent::local_search::suggest(params),
        "embed" => Ok(json!({"vector": [], "dim": 0, "model": "local-search"})),
        _ => Err(format!("search: 未知方法 {method}")),
    }
}

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
        "chat" => ok_chat_stream(params, io, req, |messages, last_is_tool| {
            let last_user = messages
                .iter()
                .rev()
                .find(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"))
                .and_then(|m| m.get("content").and_then(|c| c.as_str()))
                .unwrap_or("")
                .to_string();
            if last_user.contains("[REVIEW-GUIDE]") {
                return Ok((
                    json!({"rubric":[
                        {"id":"c1","criterion":"最终答案命中 REVIEW-FINAL 标记","check":"答案出现『REVIEW-FINAL』","must_include":"REVIEW-FINAL","weight":1.0}
                    ]})
                    .to_string(),
                    0,
                ));
            }
            if last_user.contains("[REVIEW-JUDGE]") {
                let answer = last_user
                    .split_once("待审答案：")
                    .map(|(_, a)| a)
                    .unwrap_or("");
                let pass = answer.contains("REVIEW-FINAL");
                let evidence = if pass {
                    "答案包含 REVIEW-FINAL"
                } else {
                    "答案未包含 REVIEW-FINAL"
                };
                return Ok((
                    json!({
                        "pass": pass,
                        "items": [{"id":"c1","pass":pass,"evidence":evidence}],
                        "evidence": evidence
                    })
                    .to_string(),
                    0,
                ));
            }
            if messages.iter().any(|m| {
                m.get("role").and_then(|r| r.as_str()) == Some("system")
                    && m.get("content")
                        .and_then(|c| c.as_str())
                        .map(|c| c.contains("[review_feedback]"))
                        .unwrap_or(false)
            }) {
                if last_user.contains("REVIEW_NEVER") {
                    return Ok((
                        format!("echo: {}", last_user.chars().take(60).collect::<String>()),
                        0,
                    ));
                }
                return Ok(("REVIEW-FINAL".into(), 0));
            }
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
            if let Some(rest) = last_user.split_once("SEARCH:").map(|(_, r)| r) {
                return Ok((
                    json!({"name":"web.search","arguments":{"query": rest.trim(),"count":"3"}})
                        .to_string(),
                    0,
                ));
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
    F: FnOnce(&[Value], bool) -> Result<(String, u64), String>,
{
    let messages = params
        .get("messages")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();
    let last_is_tool = messages
        .last()
        .and_then(|m| m.get("role").and_then(|r| r.as_str()))
        .map(|r| r == "tool")
        .unwrap_or(false);
    let (text, delay_ms) = f(&messages, last_is_tool)?;

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

fn provider_default_base(kind: &str) -> &'static str {
    match kind {
        "ollama" => "http://127.0.0.1:11434",
        "lmstudio" => "http://127.0.0.1:1234",
        "vllm" => "http://127.0.0.1:8000",
        _ => "http://127.0.0.1:8080",
    }
}

fn provider_caps(kind: &str) -> Vec<&'static str> {
    if kind == "ollama" {
        vec!["chat", "stream", "tools", "embed", "fim"]
    } else {
        vec!["chat", "stream", "tools", "embed"]
    }
}

/// OpenAI 兼容服务统一挂到 `/v1` 下；Ollama 原生接口仍走 `/api/*`。
fn ensure_v1(base: &str) -> String {
    let b = base.trim_end_matches('/');
    if b.ends_with("/v1") {
        b.to_string()
    } else {
        format!("{b}/v1")
    }
}

fn provider_http(
    kind: &str,
    method: &str,
    params: &Value,
    io: &Arc<Io>,
    id: u64,
) -> Result<Value, String> {
    let raw_base = params
        .get("base_url")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .unwrap_or(provider_default_base(kind))
        .trim_end_matches('/')
        .to_string();
    let openai = kind != "ollama" || raw_base.contains("/v1");
    let base = if openai {
        ensure_v1(&raw_base)
    } else {
        raw_base.clone()
    };
    let api_key = params
        .get("api_key")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|x| !x.is_empty());
    let model = params
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("qwen2.5:7b");
    let req = params.get("req").and_then(|x| x.as_u64()).unwrap_or(id);
    let label = kind;

    match method {
        "probe" => {
            // Ollama 原生列表在 /api/tags；/models 是 OpenAI 兼容路径，原生服务会 404。
            let url = if kind == "ollama" && !openai {
                format!("{base}/api/tags")
            } else {
                format!("{base}/models")
            };
            let r = net::get_json_timeout_auth(&url, std::time::Duration::from_secs(5), api_key)?;
            if r.status >= 400 {
                return Err(format!("{label} 模型列表 HTTP {}: {}", r.status, r.text()));
            }
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
            let caps = provider_caps(kind);
            Ok(json!({
                "name": label,
                "capabilities": caps,
                "base_url": base,
                "auth_configured": api_key.is_some(),
                "models": models,
            }))
        }
        "embed" => {
            let text = params.get("text").and_then(|x| x.as_str()).unwrap_or("");
            let (url, body) = if openai {
                (
                    format!("{base}/embeddings"),
                    json!({"model": model, "input": text}),
                )
            } else {
                (
                    format!("{base}/api/embed"),
                    json!({"model": model, "input": text}),
                )
            };
            let r = net::post_json_auth(&url, &body, api_key)?;
            if r.status >= 400 {
                return Err(format!("{label} embedding HTTP {}: {}", r.status, r.text()));
            }
            let v = r.json()?;
            let arr = if openai {
                v.pointer("/data/0/embedding")
                    .cloned()
                    .unwrap_or(Value::Null)
            } else {
                v.get("embeddings")
                    .and_then(|e| e.get(0))
                    .cloned()
                    .unwrap_or(Value::Null)
            };
            Ok(json!({"vector": arr, "model": model}))
        }
        "chat" => {
            let messages = params.get("messages").cloned().unwrap_or(json!([]));
            let mut body = json!({"model": model, "messages": messages, "stream": true});
            if let Some(t) = params.get("tools").cloned() {
                if t.as_array().map(|a| !a.is_empty()).unwrap_or(false) {
                    body["tools"] = t;
                }
            }
            if let Some(t) = params.get("temperature").cloned() {
                if openai {
                    body["temperature"] = t;
                } else {
                    body["options"] = json!({"temperature": t});
                }
            }
            let url = if openai {
                format!("{base}/chat/completions")
            } else {
                format!("{base}/api/chat")
            };
            let mut acc = String::new();
            let mut native_calls: Vec<Value> = Vec::new();
            let mut openai_calls: BTreeMap<u64, (String, String)> = BTreeMap::new();
            let mut upstream_finish = "stop".to_string();
            let result = net::stream_post_lines_auth(&url, &body, api_key, |line| {
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
                if let Some(e) = v.get("error") {
                    let msg = e
                        .get("message")
                        .and_then(|x| x.as_str())
                        .unwrap_or_else(|| e.as_str().unwrap_or("provider error"));
                    io.emit(req, json!({"kind": "error", "message": msg}));
                    return false;
                }
                let delta = if openai {
                    v.pointer("/choices/0/delta/content")
                        .or_else(|| v.pointer("/choices/0/message/content"))
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
                    for pointer in [
                        "/choices/0/delta/tool_calls",
                        "/choices/0/message/tool_calls",
                    ] {
                        if let Some(calls) = v.pointer(pointer).and_then(|x| x.as_array()) {
                            for c in calls {
                                let idx = c.get("index").and_then(|x| x.as_u64()).unwrap_or(0);
                                let name = c
                                    .pointer("/function/name")
                                    .and_then(|x| x.as_str())
                                    .unwrap_or("");
                                let args = c
                                    .pointer("/function/arguments")
                                    .map(|x| {
                                        x.as_str()
                                            .map(|s| s.to_string())
                                            .unwrap_or_else(|| x.to_string())
                                    })
                                    .unwrap_or_default();
                                let e = openai_calls.entry(idx).or_default();
                                if !name.is_empty() && !e.0.ends_with(name) {
                                    e.0.push_str(name);
                                }
                                e.1.push_str(&args);
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
            });
            if let Err(e) = result {
                io.emit(req, json!({"kind": "error", "message": e}));
                return Err(e);
            }
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
                let msg = format!(
                    "{label} 返回空响应（content 与 tool_calls 均为空）；已拒绝以 status=ok 静默通过。请检查 base_url/Key/代理/模型是否就绪"
                );
                io.emit(req, json!({"kind": "error", "message": msg}));
                return Err(msg);
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
            if openai {
                return Err(format!("{label}: OpenAI 兼容端点不支持原生 FIM"));
            }
            let prompt = params.get("prompt").and_then(|x| x.as_str()).unwrap_or("");
            let r = net::post_json_auth(
                &format!("{base}/api/generate"),
                &json!({"model": model, "prompt": prompt, "stream": false, "raw": true}),
                api_key,
            )?;
            if r.status >= 400 {
                return Err(format!("{label} FIM HTTP {}: {}", r.status, r.text()));
            }
            Ok(json!({"text": r.json()?.get("response").cloned().unwrap_or(Value::Null)}))
        }
        _ => Err(format!("{label}: 未知方法 {method}")),
    }
}

// ---------------- review-gate（guide / exec / judge 三角色协议） ----------------

/// review-gate 不直接调用模型：它把三角色的 prompt 与结构化解析封装成插件方法。
/// 内核按 guide → exec → judge 顺序各发一次请求，exec 看不到 rubric。
fn review_gate_plugin(method: &str, params: &Value) -> Result<Value, String> {
    match method {
        "review.guide" => {
            let input = params
                .get("input")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .chars()
                .take(8_000)
                .collect::<String>();
            let environment = params
                .get("environment")
                .and_then(|x| x.as_str())
                .unwrap_or("(未提供环境摘要)")
                .chars()
                .take(2_000)
                .collect::<String>();
            Ok(json!({"messages": json!([
                {"role": "system", "content": "你是 Review-Gate 的指导者（guide）。只写验收清单，不回答任务本身。输出一个 JSON 对象：{\"rubric\":[{\"id\":\"c1\",\"criterion\":\"可核对的要求\",\"check\":\"逐字核对 must_include 是否出现\",\"must_include\":\"必须逐字出现在最终答案中的短语\",\"weight\":1}]}。写 3–6 条；每条必须具体，禁止“准确、完整、逻辑清晰”这类空话。must_include 必须是 1–40 字的字面短语（优先结论句、文件名、命令、专有名词），judge 与 exec 都按它逐字核对；check 必须复述该字面短语，不得只写抽象要求。至少一条 must_include 命中任务的核心结论。不要输出 JSON 以外的文字。"},
                {"role": "user", "content": format!("[REVIEW-GUIDE]\n任务输入：\n{input}\n\n环境摘要：\n{environment}")}
            ]), "model_visible": true}))
        }
        "review.judge" => {
            let input = params
                .get("input")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .chars()
                .take(6_000)
                .collect::<String>();
            let answer = params
                .get("answer")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .chars()
                .take(12_000)
                .collect::<String>();
            let rubric = params.get("rubric").cloned().unwrap_or(json!([]));
            Ok(json!({"messages": json!([
                {"role": "system", "content": "你是 Review-Gate 的审批者（judge）。默认不通过；只按给定验收清单逐条核对最终答案。先逐条抄录 check 与 must_include，再在答案中查找 must_include 原文；找不到就直接 pass=false。输出一个 JSON 对象：{\"pass\":false,\"items\":[{\"id\":\"c1\",\"pass\":true,\"evidence\":\"引用答案中的原文至少 10 字\"}],\"evidence\":\"逐条核对结论，至少 10 字\"}。每条 evidence 必须引用答案原文并至少 10 字；禁止“总结”“通过”“ok”“是”等占位词。顶层 evidence 也至少 10 字。evidence 过短视为 judge 失败，会被重试。不要给总分，不要放水。"},
                {"role": "user", "content": format!("[REVIEW-JUDGE]\n用户输入：\n{input}\n\n验收清单：\n{}\n\n待审答案：\n{answer}", serde_json::to_string(&rubric).unwrap_or_default())}
            ]), "model_visible": true}))
        }
        "review.parse_rubric" => {
            let text = params.get("text").and_then(|x| x.as_str()).unwrap_or("");
            parse_rubric(text)
        }
        "review.parse_verdict" => {
            let text = params.get("text").and_then(|x| x.as_str()).unwrap_or("");
            let rubric = params.get("rubric").cloned().unwrap_or(json!([]));
            parse_verdict(text, &rubric)
        }
        "review.hardcheck" => {
            let answer = params.get("answer").and_then(|x| x.as_str()).unwrap_or("");
            let mut failures = Vec::new();
            if answer.trim().is_empty() {
                failures.push(json!({"id":"hard:non_empty","evidence":"答案为空"}));
            }
            if let Some(rubric) = params.get("rubric").and_then(|x| x.as_array()) {
                for item in rubric {
                    let id = item.get("id").and_then(|x| x.as_str()).unwrap_or("hard");
                    let criterion = item.get("criterion").cloned().unwrap_or(json!(""));
                    let check = item.get("check").cloned().unwrap_or(json!(""));
                    if let Some(lit) = item
                        .get("must_include")
                        .and_then(|x| x.as_str())
                        .filter(|x| !x.is_empty())
                    {
                        if !answer.contains(lit) {
                            failures.push(json!({
                                "id": id,
                                "criterion": criterion,
                                "check": check,
                                "evidence": format!("答案未逐字出现必须短语『{lit}』"),
                            }));
                            continue;
                        }
                    }
                    let Some(hard) = item.get("hard") else {
                        continue;
                    };
                    let kind = hard.get("kind").and_then(|x| x.as_str()).unwrap_or("");
                    let value = hard.get("value").and_then(|x| x.as_str()).unwrap_or("");
                    let ok = match kind {
                        "json" => serde_json::from_str::<Value>(answer).is_ok(),
                        "contains" => answer.contains(value),
                        _ => true,
                    };
                    if !ok {
                        failures.push(json!({
                            "id": id,
                            "criterion": criterion,
                            "check": check,
                            "evidence": format!("hardcheck {kind} 未通过"),
                        }));
                    }
                }
            }
            Ok(json!({"pass": failures.is_empty(), "failed": failures}))
        }
        _ => Err(format!("review-gate: 未知方法 {method}")),
    }
}

fn json_slice(text: &str) -> &str {
    let t = text.trim();
    let t = t.strip_prefix("```json").unwrap_or(t);
    let t = t.strip_prefix("```").unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t).trim();
    if let (Some(a), Some(b)) = (t.find('{'), t.rfind('}')) {
        if b > a {
            return &t[a..=b];
        }
    }
    t
}

fn quoted_literal(check: &str) -> Option<String> {
    for (open, close) in [
        ('『', '』'),
        ('「', '」'),
        ('`', '`'),
        ('“', '”'),
        ('"', '"'),
    ] {
        if let Some(a) = check.find(open) {
            let rest = &check[a + open.len_utf8()..];
            if let Some(b) = rest.find(close) {
                let lit = rest[..b].trim();
                if !lit.is_empty() {
                    return Some(lit.to_string());
                }
            }
        }
    }
    None
}

fn parse_rubric(text: &str) -> Result<Value, String> {
    let v: Value = serde_json::from_str(json_slice(text))
        .map_err(|e| format!("验收清单不是合法 JSON: {e}"))?;
    let arr = v
        .get("rubric")
        .or_else(|| v.get("items"))
        .and_then(|x| x.as_array())
        .or_else(|| v.as_array())
        .ok_or("验收清单缺少 rubric 数组")?;
    if arr.is_empty() || arr.len() > 12 {
        return Err(format!("验收清单条数必须在 1–12 之间，实际 {}", arr.len()));
    }
    let mut out = Vec::new();
    for (i, item) in arr.iter().enumerate() {
        let criterion = item
            .get("criterion")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim();
        let check = item
            .get("check")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim();
        if criterion.is_empty() || check.is_empty() {
            return Err(format!("验收项 {} 缺少 criterion/check", i + 1));
        }
        let must_include = item
            .get("must_include")
            .and_then(|x| x.as_str())
            .map(|x| {
                x.trim()
                    .trim_matches(|c| {
                        matches!(c, '『' | '』' | '「' | '」' | '`' | '"' | '“' | '”')
                    })
                    .trim()
                    .to_string()
            })
            .filter(|x| !x.is_empty())
            .or_else(|| quoted_literal(check));
        let Some(must_include) = must_include else {
            return Err(format!(
                "验收项 {} 的 check 不是字面判据：必须提供 must_include 或在 check 中用『』标出应逐字出现的短语",
                i + 1
            ));
        };
        if must_include.chars().count() > 40 {
            return Err(format!("验收项 {} 的 must_include 超过 40 字", i + 1));
        }
        let id = item
            .get("id")
            .and_then(|x| x.as_str())
            .filter(|x| !x.trim().is_empty())
            .map(|x| x.to_string())
            .unwrap_or_else(|| format!("c{}", i + 1));
        let weight = item.get("weight").and_then(|x| x.as_f64()).unwrap_or(1.0);
        out.push(json!({
            "id": id,
            "criterion": criterion,
            "check": check,
            "must_include": must_include,
            "weight": weight.clamp(0.1, 10.0),
            "hard": item.get("hard").cloned(),
        }));
    }
    Ok(json!({"rubric": out, "count": out.len()}))
}

fn parse_verdict(text: &str, rubric: &Value) -> Result<Value, String> {
    let v: Value = serde_json::from_str(json_slice(text))
        .map_err(|e| format!("审批结果不是合法 JSON: {e}"))?;
    let expected: Vec<String> = rubric
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| x.get("id").and_then(|y| y.as_str()).map(|s| s.to_string()))
        .collect();
    if expected.is_empty() {
        return Err("审批结果没有对应验收项".into());
    }
    let items = v
        .get("items")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let mut normalized = Vec::new();
    let mut failed = Vec::new();
    for id in &expected {
        let found = items
            .iter()
            .find(|x| x.get("id").and_then(|y| y.as_str()) == Some(id.as_str()));
        let Some(item) = found else {
            return Err(format!("审批结果缺少验收项 {id}"));
        };
        let pass = item.get("pass").and_then(|x| x.as_bool()).unwrap_or(false);
        let evidence = item
            .get("evidence")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        // evidence 太短说明 judge 只是在占位（例如“总结”），属 judge 失败，交给上层重试。
        if evidence.chars().count() < 10 {
            return Err(format!(
                "审批项 {id} 的 evidence 过短（少于 10 字）：{evidence:?}"
            ));
        }
        if !pass {
            failed.push(Value::String(id.clone()));
        }
        let meta = rubric
            .as_array()
            .into_iter()
            .flatten()
            .find(|x| x.get("id").and_then(|y| y.as_str()) == Some(id.as_str()));
        let criterion = meta
            .and_then(|x| x.get("criterion"))
            .cloned()
            .unwrap_or(json!(""));
        let check = meta
            .and_then(|x| x.get("check"))
            .cloned()
            .unwrap_or(json!(""));
        normalized.push(json!({
            "id": id,
            "pass": pass,
            "evidence": evidence,
            "criterion": criterion,
            "check": check,
        }));
    }
    let summary = v
        .get("evidence")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim();
    if summary.chars().count() < 10 {
        return Err(format!("审批总结 evidence 过短（少于 10 字）：{summary:?}"));
    }
    let pass = failed.is_empty() && v.get("pass").and_then(|x| x.as_bool()).unwrap_or(false);
    Ok(json!({
        "pass": pass,
        "items": normalized,
        "failed": failed,
        "evidence": summary,
    }))
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
        return (workspace.to_path_buf(), ".".to_string());
    }
    if let Some(rest) = norm.strip_prefix("workspace/") {
        norm = rest.to_string();
    }
    // 项目级工作区：模型有时会把当前项目名写进 path（proj-a/hello.txt）。
    // 路径已经以项目根为基准，再叠一层会变成 proj-a/proj-a/hello.txt。
    // 只去掉「当前项目名 + 路径分隔符」这一精确前缀；同名子目录不能被访问，
    // 但这是可接受的歧义消解，文档与 schema 已明确“path 相对项目根”。
    if let Some(project) = workspace.file_name().and_then(|x| x.to_str()) {
        let prefix = format!("{project}/");
        if norm.len() > prefix.len() && norm.starts_with(&prefix) {
            norm = norm[prefix.len()..].to_string();
        }
    }
    (workspace.to_path_buf(), norm)
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
        for p in [
            "proj/b.txt",
            "workspace/proj/b.txt",
            "/workspace/proj/b.txt",
        ] {
            let (base, rel) = tool_base(project, p);
            assert_eq!(base, project, "base for {p}");
            assert_eq!(rel, "b.txt", "project prefix should be removed for {p}");
        }
        // 非项目名前缀不应被误删
        let (_, rel) = tool_base(project, "proj-data/c.txt");
        assert_eq!(rel, "proj-data/c.txt");
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

    #[test]
    fn review_rubric_requires_literal_must_include() {
        let ok = parse_rubric("```json\n{\"rubric\":[{\"id\":\"c1\",\"criterion\":\"包含结论\",\"check\":\"答案第一段有『REVIEW-FINAL』\",\"must_include\":\"REVIEW-FINAL\"}]}\n```").unwrap();
        assert_eq!(ok["count"], 1);
        assert_eq!(ok["rubric"][0]["id"], "c1");
        assert_eq!(ok["rubric"][0]["must_include"], "REVIEW-FINAL");
        let quoted = parse_rubric(
            "{\"rubric\":[{\"id\":\"c1\",\"criterion\":\"包含结论\",\"check\":\"答案出现『REVIEW-FINAL』\"}]}",
        )
        .unwrap();
        assert_eq!(quoted["rubric"][0]["must_include"], "REVIEW-FINAL");
        let bad = parse_rubric("{\"rubric\":[{\"criterion\":\"只有标题\",\"check\":\"\"}]}");
        assert!(bad.is_err());
        let vague = parse_rubric(
            "{\"rubric\":[{\"id\":\"c1\",\"criterion\":\"结论完整\",\"check\":\"答案逻辑清晰\"}]}",
        );
        assert!(vague.is_err());
    }

    #[test]
    fn review_verdict_short_evidence_is_rejected() {
        let rubric = json!([
            {"id":"c1","criterion":"包含结论","check":"答案出现『REVIEW-FINAL』"},
            {"id":"c2","criterion":"包含文件","check":"答案出现『w1.txt』"}
        ]);
        let short = parse_verdict(
            "{\"pass\":true,\"items\":[{\"id\":\"c1\",\"pass\":true,\"evidence\":\"总结\"},{\"id\":\"c2\",\"pass\":true,\"evidence\":\"答案中出现 w1.txt 路径\"}],\"evidence\":\"总结\"}",
            &rubric,
        );
        assert!(short.is_err());
        let ok = parse_verdict(
            "{\"pass\":true,\"items\":[{\"id\":\"c1\",\"pass\":true,\"evidence\":\"答案第一段出现 REVIEW-FINAL 标记\"},{\"id\":\"c2\",\"pass\":true,\"evidence\":\"答案写明 workspace/w1.txt 路径\"}],\"evidence\":\"逐条核对通过，答案包含两个必须短语\"}",
            &rubric,
        )
        .unwrap();
        assert_eq!(ok["pass"], true);
        assert_eq!(ok["items"][0]["criterion"], "包含结论");
        assert_eq!(ok["items"][0]["check"], "答案出现『REVIEW-FINAL』");
    }

    #[test]
    fn review_hardcheck_requires_literal_must_include() {
        let rubric = json!([{
            "id":"c1",
            "criterion":"包含结论",
            "check":"答案出现『REVIEW-FINAL』",
            "must_include":"REVIEW-FINAL"
        }]);
        let bad = review_gate_plugin(
            "review.hardcheck",
            &json!({"answer":"这里只有一段普通总结", "rubric":rubric}),
        )
        .unwrap();
        assert_eq!(bad["pass"], false);
        assert!(bad["failed"][0]["evidence"]
            .as_str()
            .unwrap()
            .contains("REVIEW-FINAL"));
        let ok = review_gate_plugin(
            "review.hardcheck",
            &json!({"answer":"最终答案 REVIEW-FINAL", "rubric":rubric}),
        )
        .unwrap();
        assert_eq!(ok["pass"], true);
    }

    #[test]
    fn review_guide_and_judge_prompts_are_role_separated() {
        let guide =
            review_gate_plugin("review.guide", &json!({"input":"任务", "environment":"ws"}))
                .unwrap();
        let msgs = guide["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["role"], "system");
        assert!(msgs[1]["content"]
            .as_str()
            .unwrap()
            .contains("[REVIEW-GUIDE]"));
        let judge = review_gate_plugin(
            "review.judge",
            &json!({"input":"任务", "answer":"答案", "rubric":[{"id":"c1"}]}),
        )
        .unwrap();
        let msgs = judge["messages"].as_array().unwrap();
        assert!(msgs[1]["content"]
            .as_str()
            .unwrap()
            .contains("[REVIEW-JUDGE]"));
        assert!(msgs[1]["content"].as_str().unwrap().contains("待审答案"));
    }

    #[test]
    fn ninfer_probe_uses_v1_and_bearer_key() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let _ = tx.send(String::from_utf8_lossy(&buf[..n]).to_string());
                let _ = s.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 25\r\n\r\n{\"data\":[{\"id\":\"local\"}]}",
                );
            }
        });
        let io = Arc::new(Io {
            out: Mutex::new(std::io::stdout()),
            codec: Codec::MsgPack,
            cancelled: Mutex::new(HashSet::new()),
        });
        let v = provider_http(
            "ninfer",
            "probe",
            &json!({"base_url": format!("http://{addr}"), "api_key": "sk-ninfer"}),
            &io,
            1,
        )
        .unwrap();
        let req = rx.recv().unwrap();
        assert!(req.starts_with("GET /v1/models "), "{req}");
        assert!(req.contains("Authorization: Bearer sk-ninfer"), "{req}");
        assert_eq!(v["models"][0], "local");
        assert_eq!(v["auth_configured"], true);
    }

    #[test]
    fn ollama_probe_uses_api_tags_and_reads_native_models() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let _ = tx.send(String::from_utf8_lossy(&buf[..n]).to_string());
                let _ = s.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 62\r\n\r\n{\"models\":[{\"name\":\"qwen3.5:4b\"},{\"name\":\"qwen2.5-coder:7b\"}]}",
                );
            }
        });
        let io = Arc::new(Io {
            out: Mutex::new(std::io::stdout()),
            codec: Codec::MsgPack,
            cancelled: Mutex::new(HashSet::new()),
        });
        let v = provider_http(
            "ollama",
            "probe",
            &json!({"base_url": format!("http://{addr}"), "model": "qwen3.5:4b"}),
            &io,
            1,
        )
        .unwrap();
        let req = rx.recv().unwrap();
        assert!(req.starts_with("GET /api/tags "), "{req}");
        assert_eq!(v["models"][0], "qwen3.5:4b");
        assert_eq!(v["models"][1], "qwen2.5-coder:7b");
    }

    #[test]
    fn openai_compatible_base_normalization() {
        assert_eq!(
            ensure_v1("http://127.0.0.1:8080"),
            "http://127.0.0.1:8080/v1"
        );
        assert_eq!(
            ensure_v1("http://127.0.0.1:8080/v1/"),
            "http://127.0.0.1:8080/v1"
        );
    }

    fn temp_root(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("fa-plugin-{}-{}", std::process::id(), tag));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
