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

pub(crate) fn main() {
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
        // 设置项的唯一声明处是 plugins/<name>/plugin.json：内核在安装时读取并转述给前端，
        // 握手不重复声明，避免两处漂移。
        settings: None,
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
                // 复现用夹具：先只给“描述项”，再看内核是否把错误回注进第二次请求。
                if last_user.contains("REVIEW_SOFT_FIRST")
                    && !last_user.contains("[上一次的判据不合格")
                {
                    return Ok((
                        json!({"rubric":[
                            {"id":"c1","criterion":"最终答案命中 REVIEW-FINAL 标记","check":"读答案判断"},
                            {"id":"c2","criterion":"答案不含占位词","check":"读答案判断"}
                        ]})
                        .to_string(),
                        0,
                    ));
                }
                if last_user.contains("REVIEW_SOFT_ONLY") {
                    return Ok((
                        json!({"rubric":[
                            {"id":"c1","criterion":"最终答案命中 REVIEW-FINAL 标记","check":"读答案判断"},
                            {"id":"c2","criterion":"答案不含占位词","check":"读答案判断"}
                        ]})
                        .to_string(),
                        0,
                    ));
                }
                if last_user.contains("REVIEW_MINPOINTS") {
                    return Ok((
                        json!({"rubric":[
                            {"id":"c1","criterion":"答案命中标记","check":"出现『REVIEW-FINAL』","must_include":"REVIEW-FINAL"},
                            {"id":"c2","criterion":"至少两个要点","check":"用分号或列表分点","min_points":2},
                            {"id":"c3","criterion":"答案是否说明了用途","check":"读答案判断"}
                        ]})
                        .to_string(),
                        0,
                    ));
                }
                if last_user.contains("REVIEW_K1") {
                    return Ok((
                        json!({"rubric":[
                            {"id":"c1","criterion":"答案不含占位词","check":"不得出现 TODO/FIXME","must_not_include":["TODO","FIXME"]},
                            {"id":"c3","criterion":"答案是否说明了用途","check":"读答案判断"}
                        ]})
                        .to_string(),
                        0,
                    ));
                }
                if last_user.contains("REVIEW_WRITE") {
                    return Ok((
                        json!({"rubric":[
                            {"id":"c1","criterion":"notes.txt 存在且包含三条内存管理要点，每条以 - 开头","check":"核对文件内容、行数与行首格式","must_include":["-"],"min_points":3,"pattern":"(?m)^- "},
                            {"id":"c2","criterion":"使用 tool.fs.write 写入文件","check":"核对写入成功记录与文件正文"}
                        ]})
                        .to_string(),
                        0,
                    ));
                }
                if last_user.contains("REVIEW_M1") {
                    return Ok((
                        json!({"rubric":[
                            {"id":"c1","criterion":"答案命中标记","check":"出现『REVIEW-FINAL』","must_include":"REVIEW-FINAL"},
                            {"id":"c2","criterion":"禁止使用示例中的占位词","check":"检查答案中是否出现 <...> 占位符","must_not_include":["<...>"]},
                            {"id":"c3","criterion":"至少两个要点","check":"用分号或列表分点","min_points":2}
                        ]})
                        .to_string(),
                        0,
                    ));
                }
                if last_user.contains("REVIEW_SUBJ") {
                    return Ok((
                        json!({"rubric":[
                            {"id":"c1","criterion":"答案命中标记","check":"出现『REVIEW-FINAL』","must_include":"REVIEW-FINAL"},
                            {"id":"c4","criterion":"判断答案是否简洁明了","check":"读答案判断内容是否清晰、简洁"}
                        ]})
                        .to_string(),
                        0,
                    ));
                }
                if last_user.contains("REVIEW_MISSING_FIRST") {
                    return Ok((
                        json!({"rubric":[
                            {"id":"c1","criterion":"最终答案命中 REVIEW-FINAL 标记","check":"答案出现『REVIEW-FINAL』","must_include":"REVIEW-FINAL"},
                            {"id":"c3","criterion":"答案是否说明了用途","check":"读答案判断"},
                            {"id":"c4","criterion":"答案是否给出例子","check":"读答案判断"}
                        ]})
                        .to_string(),
                        0,
                    ));
                }
                return Ok((
                    json!({"rubric":[
                        {"id":"c1","criterion":"最终答案命中 REVIEW-FINAL 标记","check":"答案出现『REVIEW-FINAL』","must_include":"REVIEW-FINAL","weight":1.0},
                        {"id":"c2","criterion":"答案不含被禁止的占位词","check":"不得出现『绝不应出现的占位词』","must_not_include":["绝不应出现的占位词"],"weight":1.0},
                        {"id":"c3","criterion":"答案是否说明了用途","check":"读答案判断"}
                    ]})
                    .to_string(),
                    0,
                ));
            }
            // judge 重试时最后一条 user 可能是内核追加的“审批结果不合格”回注，
            // 所以要从整段 messages 里找原始的 [REVIEW-JUDGE] 上下文。
            let judge_text = messages
                .iter()
                .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
                .find(|c| c.contains("[REVIEW-JUDGE]"));
            if let Some(judge_text) = judge_text {
                let (meta, answer) = judge_text
                    .split_once("待审答案：")
                    .unwrap_or((judge_text, ""));
                let answer = answer.trim();
                let inventory = meta
                    .split_once("验收清单：")
                    .map(|(_, x)| x.trim())
                    .unwrap_or("[]");
                let rubric: Value = serde_json::from_str(inventory).unwrap_or(json!([]));
                let ids: Vec<String> = rubric
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| {
                                x.get("id").and_then(|y| y.as_str()).map(|s| s.to_string())
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let ids = if ids.is_empty() {
                    vec!["c1".to_string()]
                } else {
                    ids
                };
                let quote: String = answer.chars().take(24).collect();
                let file_evidence =
                    answer.contains("tool.fs.write path=") && answer.contains("---");
                let mut items = Vec::new();
                for id in &ids {
                    // 模拟 K1：若负向判据被送进 judge，7B 只能写出“答案中没有…”，
                    // 这不是答案原文，会被 parse_verdict 拒绝。路由正确时 c1 不会出现在这里。
                    if rubric
                        .as_array()
                        .map(|a| {
                            a.iter().any(|x| {
                                x.get("id").and_then(|y| y.as_str()) == Some(id.as_str())
                                    && x.get("must_not_include").is_some()
                            })
                        })
                        .unwrap_or(false)
                    {
                        items.push(json!({
                            "id": id,
                            "pass": true,
                            "evidence": "答案中没有出现 TODO 或 FIXME 等占位词。"
                        }));
                    } else {
                        items.push(json!({
                            "id": id,
                            "pass": answer.contains("REVIEW-FINAL") || file_evidence,
                            "evidence": quote
                        }));
                    }
                }
                // K2 夹具：第一次故意漏掉最后一项，带“审批结果不合格”回注后补齐。
                let retried = messages.iter().any(|m| {
                    m.get("content")
                        .and_then(|c| c.as_str())
                        .map(|c| c.contains("[审批结果不合格]"))
                        .unwrap_or(false)
                });
                if judge_text.contains("REVIEW_MISSING_FIRST") && !retried && items.len() > 1 {
                    items.pop();
                }
                let pass = items
                    .iter()
                    .all(|x| x.get("pass").and_then(|y| y.as_bool()).unwrap_or(false));
                return Ok((
                    json!({"pass": pass, "items": items, "evidence": quote}).to_string(),
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
            if last_is_tool && last_user.contains("REVIEW_WRITE_BAD") {
                return Err("XML syntax error on line 3".into());
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
            if last_user.contains("REVIEW_MINPOINTS") {
                // J1 回归：命中 must_include，但只有一段、没有分号/列表，机器计数 1 < 2。
                return Ok(("REVIEW-FINAL".into(), 0));
            }
            if last_user.contains("REVIEW_M1") || last_user.contains("REVIEW_SUBJ") {
                return Ok(("REVIEW-FINAL".into(), 0));
            }
            if last_user.contains("REVIEW_K1") || last_user.contains("REVIEW_MISSING_FIRST") {
                return Ok(("REVIEW-FINAL".into(), 0));
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
                    // N15：Ollama 对 XML 风格工具调用会先报 XML syntax error；
                    // 若已累计到可解析的调用文本，就从文本恢复，不让整轮直接失败。
                    if acc.contains("<function") {
                        if let Some(c) = featheragent::context::parse::extract(&acc)
                            .into_iter()
                            .next()
                        {
                            native_calls.push(json!({
                                "name": c.name,
                                "arguments": c.arguments
                            }));
                            upstream_finish = "tool_calls".into();
                            return false;
                        }
                    }
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
                // HTTP 500 的错误体可能只有 XML syntax error，生成内容没有进入回调；
                // 若回调已收到 function 文本，仍先从累计文本抢救调用。
                let recovered = if acc.contains("<function") {
                    featheragent::context::parse::extract(&acc)
                        .into_iter()
                        .next()
                } else {
                    None
                };
                if let Some(c) = recovered {
                    native_calls.push(json!({
                        "name": c.name,
                        "arguments": c.arguments
                    }));
                    upstream_finish = "tool_calls".into();
                } else {
                    io.emit(req, json!({"kind": "error", "message": e}));
                    return Err(e);
                }
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
            // 上一次判据不合格的原因：原样回注，避免“重发同一个 prompt 得到同一个错”。
            let feedback = params
                .get("feedback")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .chars()
                .take(600)
                .collect::<String>();
            Ok(json!({"messages": json!([
                {"role": "system", "content": GUIDE_SYSTEM},
                {"role": "user", "content": guide_user_prompt(&input, &environment, &feedback)}
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
                {"role": "system", "content": "你是 Review-Gate 的审批者（judge）。你只审消息里给出的这些判据，不要引入未给出的条目。带 judge_only:true 的条目是描述型（只有 criterion/check）：按 criterion 判断答案是否满足，并给答案原文作 evidence。带 must_include / must_not_include / min_points / pattern / regex 的确定性条目已由 hardcheck 判定；只有在 judge_only 模式下它们才会出现在这里，此时按硬键核对。必须逐项返回，id 一个都不能少，漏项视为不合格。输出一个 JSON 对象：{\"pass\":false,\"items\":[{\"id\":\"c1\",\"pass\":true,\"evidence\":\"答案中的连续原文\"}],\"evidence\":\"答案中的连续原文\"}。每条 evidence 必须逐字复制答案里连续的至少 10 个字，禁止改写、禁止抄提示词、禁止用“总结/通过/ok”占位；顶层 evidence 同样要求。evidence 不在答案里出现视为 judge 失败并重试。不要给总分，不要放水。"},
                {"role": "user", "content": format!("[REVIEW-JUDGE]\n用户输入：\n{input}\n\n验收清单：\n{}\n\n待审答案：\n{answer}", serde_json::to_string(&rubric).unwrap_or_default())}
            ]), "model_visible": true}))
        }
        "review.parse_rubric" => {
            let text = params.get("text").and_then(|x| x.as_str()).unwrap_or("");
            let input = params.get("input").and_then(|x| x.as_str()).unwrap_or("");
            parse_rubric_with_input(text, input)
        }
        "review.parse_verdict" => {
            let text = params.get("text").and_then(|x| x.as_str()).unwrap_or("");
            let rubric = params.get("rubric").cloned().unwrap_or(json!([]));
            let answer = params.get("answer").and_then(|x| x.as_str()).unwrap_or("");
            parse_verdict(text, &rubric, answer)
        }
        "review.hardcheck" => {
            let answer = params.get("answer").and_then(|x| x.as_str()).unwrap_or("");
            let pattern_engine = params
                .get("pattern_engine")
                .and_then(|x| x.as_str())
                .unwrap_or("glob");
            let mut failures = Vec::new();
            // min_points 是“形态判据”：中文答案常用分号/加粗小标题行内枚举，
            // 纯计数容易误杀。这里把它降级为软判据，只告警，不直接判死。
            let mut soft = Vec::new();
            if answer.trim().is_empty() {
                failures.push(json!({"id":"hard:non_empty","evidence":"答案为空"}));
            }
            if let Some(rubric) = params.get("rubric").and_then(|x| x.as_array()) {
                for item in rubric {
                    let id = item.get("id").and_then(|x| x.as_str()).unwrap_or("hard");
                    let criterion = item.get("criterion").cloned().unwrap_or(json!(""));
                    let check = item.get("check").cloned().unwrap_or(json!(""));
                    let mut fail = |why: String| {
                        failures.push(json!({
                            "id": id,
                            "criterion": criterion,
                            "check": check,
                            "evidence": why,
                        }));
                    };
                    let musts: Vec<String> = match item.get("must_include") {
                        Some(Value::String(s)) => vec![s.clone()],
                        Some(Value::Array(a)) => a
                            .iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect(),
                        _ => Vec::new(),
                    };
                    if !musts.is_empty() {
                        // N5：数组不再默认“全部命中”。默认 min（至少 ceil(n/2) 项），
                        // 显式 all/any/min 由 match + min_matches 决定。
                        let mode = item
                            .get("match")
                            .and_then(|x| x.as_str())
                            .unwrap_or(if musts.len() > 1 { "min" } else { "all" });
                        let need = match mode {
                            "all" => musts.len(),
                            "any" => 1,
                            "min" => item
                                .get("min_matches")
                                .and_then(featheragent::util::json_u64)
                                .unwrap_or(musts.len().div_ceil(2) as u64)
                                as usize,
                            _ => musts.len(),
                        };
                        let need = need.clamp(1, musts.len());
                        let hits = musts
                            .iter()
                            .filter(|lit| {
                                include_terms(lit)
                                    .iter()
                                    .all(|term| answer.contains(term.as_str()))
                            })
                            .count();
                        if hits < need {
                            // L2：feedback 会把 evidence 原样交给 exec。若这里回显字面短语，
                            // 模型会直接把该短语贴到答案末尾“补标签”，内容却没有改善。
                            // 只说明“缺一项必须覆盖的要求”，不给出字面值。
                            fail(format!(
                                "答案未覆盖必须内容：只出现 {hits}/{} 项，判据「{criterion}」要求 match={mode}（至少 {need} 项）",
                                musts.len()
                            ));
                            continue;
                        }
                    }
                    let banned: Vec<String> = match item.get("must_not_include") {
                        Some(Value::String(s)) => vec![s.clone()],
                        Some(Value::Array(a)) => a
                            .iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect(),
                        _ => Vec::new(),
                    };
                    if let Some(hit) = banned
                        .iter()
                        .find(|b| !b.is_empty() && answer.contains(b.as_str()))
                    {
                        fail(format!("答案出现了被禁止的内容『{hit}』"));
                        continue;
                    }
                    if let Some(n) = item.get("min_points").and_then(|x| x.as_u64()) {
                        let got = count_points(answer) as u64;
                        if got < n {
                            soft.push(json!({
                                "id": id,
                                "criterion": criterion,
                                "check": check,
                                "kind": "min_points",
                                "points": got,
                                "need": n,
                                "evidence": format!("机器计数 {got} 个要点，判据要求至少 {n} 个；请按答案实际内容复核"),
                            }));
                            continue;
                        }
                    }
                    if let Some(pat) = item.get("pattern").and_then(|x| x.as_str()) {
                        if !pat.trim().is_empty() {
                            let ok = if pattern_engine == "regex" {
                                regex::Regex::new(pat)
                                    .map(|r| r.is_match(answer))
                                    .unwrap_or(false)
                            } else {
                                glob_match(pat, answer)
                            };
                            if !ok {
                                fail(format!("答案未匹配 pattern『{pat}』"));
                                continue;
                            }
                        }
                    }
                    if let Some(pat) = item.get("regex").and_then(|x| x.as_str()) {
                        let min = item
                            .get("min_matches")
                            .and_then(|x| x.as_u64())
                            .unwrap_or(1) as usize;
                        match regex::Regex::new(pat) {
                            Ok(re) => {
                                let got = re.find_iter(answer).count();
                                if got < min {
                                    fail(format!(
                                        "答案正则匹配 {got} 次，少于要求的 {min} 次：『{pat}』"
                                    ));
                                    continue;
                                }
                            }
                            Err(e) => {
                                fail(format!("regex 编译失败『{pat}』：{e}"));
                                continue;
                            }
                        }
                    }
                    if let Some(hard) = item.get("hard") {
                        let kind = hard.get("kind").and_then(|x| x.as_str()).unwrap_or("");
                        let value = hard.get("value").and_then(|x| x.as_str()).unwrap_or("");
                        let ok = match kind {
                            "json" => serde_json::from_str::<Value>(answer).is_ok(),
                            "contains" => answer.contains(value),
                            _ => true,
                        };
                        if !ok {
                            fail(format!("hardcheck {kind} 未通过"));
                        }
                    }
                }
            }
            Ok(json!({"pass": failures.is_empty(), "failed": failures, "soft": soft}))
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

/// 通配匹配：`*` 任意字符序列、`?` 单个字符、`|` 分隔多个模式；大小写不敏感。
/// 刻意不引入 regex 依赖：内核体积门禁要求零新增依赖，通配已足够覆盖判据。
fn glob_match(pattern: &str, text: &str) -> bool {
    pattern.split('|').any(|p| {
        let p = p.trim();
        glob_one(p, text) || {
            // pattern 是“答案里出现一段匹配文本”，不是整段答案匹配：
            // 对答案每个子串起点试一次，覆盖 `workspace/*.txt` 这类路径模式。
            let chars: Vec<char> = text.chars().collect();
            (0..chars.len()).rev().any(|start| {
                let tail: String = chars[start..].iter().collect();
                glob_one_prefix(p, &tail)
            })
        }
    })
}

/// 只要求 pattern 匹配 text 的某个前缀，用于子串搜索。
fn glob_one_prefix(pat: &str, text: &str) -> bool {
    glob_one(&format!("{pat}*"), text)
}

fn glob_one(pat: &str, text: &str) -> bool {
    let p: Vec<char> = pat.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (usize::MAX, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = pi;
            pi += 1;
            mark = ti;
        } else if star != usize::MAX {
            pi = star + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// 统计答案里的要点数：markdown 列表、数字编号、中文顿号编号都算。
fn count_points(answer: &str) -> usize {
    // 要点识别要和中文答案的真实形态对齐：
    // 1) 行首列表标记（- * + • · 数字编号）；2) 行首加粗小标题；3) 分号分段；4) 空行分段。
    // 取四种口径的最大值，宁可宽松也不误杀；min_points 失败已降级为软判据交 judge 复核。
    let mut markered = 0usize;
    for line in answer.lines() {
        let l = line.trim_start();
        if ["- ", "* ", "+ ", "• ", "· ", "-\t"]
            .iter()
            .any(|p| l.starts_with(p))
        {
            markered += 1;
            continue;
        }
        let digits = l.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0
            && l[digits..]
                .trim_start()
                .starts_with(['.', '、', '．', ')', '）'])
        {
            markered += 1;
            continue;
        }
        // `**小标题**：内容` 这种行也很常见，算一个要点。
        if l.starts_with("**") && l[2..].contains("**") {
            markered += 1;
        }
    }
    let clauses = answer
        .split(['；', ';'])
        .filter(|s| s.trim().chars().filter(|c| !c.is_whitespace()).count() >= 6)
        .count();
    let paras = answer
        .split("\n\n")
        .filter(|b| b.trim().chars().count() >= 12)
        .count();
    markered.max(clauses).max(paras).min(12)
}

/// 判据不能是整句：含句末标点或以句号结尾会让门禁退化成“抄指导者的句子”。
fn sentence_like(s: &str) -> bool {
    let t = s.trim();
    if t.ends_with(['。', '！', '？', '!', '?', '.']) || t.contains(['。', '！', '？']) {
        return true;
    }
    // 没有句末标点但带“是/的/了/和/并/而/为”这类连接词、且够长，也当整句处理：
    // 4B 模型最爱的写法就是“X 是 Y 的 Z”，把这种当关键词等于把答案写进判据。
    t.chars().count() >= 12
        && ["是", "的", "了", "和", "并", "而", "为", "把", "将", "以及"]
            .iter()
            .any(|m| t.contains(m))
}

/// 归一化答案与 evidence：去掉 markdown 标记并压缩空白，再做连续子串比对。
fn quote_norm(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '*' | '`' | '#' | '>' | '_' | '~'))
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("")
}

/// evidence 必须能在答案里找到连续 ≥10 字（答案不足 10 字时按答案长度算）。
fn evidence_quotes(answer: &str, evidence: &str) -> bool {
    let a: Vec<char> = quote_norm(answer).chars().collect();
    let e: Vec<char> = quote_norm(evidence).chars().collect();
    if a.is_empty() {
        return true;
    }
    let need = 10.min(a.len());
    if e.len() < need {
        return false;
    }
    for i in 0..=(e.len() - need) {
        if a.windows(need).any(|w| w == &e[i..i + need]) {
            return true;
        }
    }
    false
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

/// guide 的系统提示：schema + few-shot + 明确“criterion/check 只是描述”。
/// rc.3 真机复测里 7B 一直沿用 rc.2 的 {criterion, check} 形状，这里必须把键名讲死。
const GUIDE_SYSTEM: &str = r#"你是 Review-Gate 的指导者（guide）。你只写“可核对的验收判据”，绝对不写答案、结论或完整句子。

输出一个 JSON 对象，结构固定。__FILL_*__ 只是字段占位符，不是判据内容：
{"rubric":[
  {"id":"c1","criterion":"__FILL_CRITERION__","check":"__FILL_CHECK__","must_include":"__FILL_PHRASE__"},
  {"id":"c2","criterion":"__FILL_FORBIDDEN_REASON__","check":"__FILL_FORBIDDEN_CHECK__","must_not_include":["__FILL_WORD__"]},
  {"id":"c3","criterion":"__FILL_STRUCTURE_REASON__","check":"__FILL_STRUCTURE_CHECK__","min_points":"__FILL_COUNT__"},
  {"id":"c4","criterion":"__FILL_JUDGE_REASON__","check":"__FILL_JUDGE_CHECK__"}
]}

规则（必须遵守）：
1. 写 1–5 条判据，每条都要针对下面的“任务输入”，不要复用示例的领域或文字。
2. criterion 和 check 只是给人看的说明，不参与判定；普通任务真正的判据只能写在 must_include / must_not_include / min_points / pattern / regex 这五个键上。写文件任务例外：只写 criterion/check，不要写任何硬键。
3. 普通任务整份清单至少有一条判据用到这五个硬键中的任意一个，否则整份清单作废。写文件任务跳过本规则，全部判据交给 judge 核对文件。
4. must_include 只能写任务里必须出现的专有名词、术语、文件名、命令或固定写法，2–12 字；禁止写概念性描述或句子，禁止含“和/与/及/或/是/的”等连接词。概念要求（如“说明作用”“解释过程”）不要用 must_include，写在 criterion/check 交给 judge。多个术语用数组时，默认 match=min（至少 ceil(n/2) 项）；功能清单/举例用 match=min 或 any，只有“SYN 与 ACK 都必须出现”这类硬约束才写 match=all；需要精确数量时写 min_matches。例如 must_include 用 “SYN”“ACK”“操作系统”“进程调度”，不要写“管理计算机硬件和资源”。禁止使用 REVIEW-FINAL、REVIEW-START 这类测试标记；禁止保留 <...> 占位符。写文件任务不要使用本键。
5. 普通任务至少一条使用 must_not_include（禁止出现的词）、min_points（至少几个要点）、pattern（通配：* 任意、? 单字符、| 或；或 regex 引擎下的正则）或 regex（真正则，可配 min_matches）。判据要能区分“答对”和“照抄判据”。写文件任务跳过本规则。
5b. 普通任务至少留一条“描述型”判据：只写 criterion/check，不写任何硬键，交给 judge 评估。描述型不要超过 2 条。写文件任务全部判据都是描述型。
6. 判据必须是任务本身可验证的事实（结论关键词、文件名、命令、路径、格式），不要评价“逻辑清晰”“内容完整”这类无法核对的话。
7. criterion 和 check 必须写出这条判据的具体含义；禁止照抄示例里的占位符或“这项检查什么”“怎么核对”这类空话。
8. 示例只示范字段名与结构。不要在判据里讨论占位符、示例、模板本身，不要把 __FILL_*__ 写进 must_not_include；不要写“禁止使用占位符”这类自我指涉的条目。
9. 不要写主观判据（简洁、清晰、明了、美观、通顺、易懂、逻辑性强、排版好看、术语准确、表达准确、内容完整）。主观要求只写在 criterion/check，不写 must_not_include；不要把“软件/硬件/计算机”这类领域常用词当禁词。需要篇幅或格式时，用 min_points / pattern / regex 等可计数、可核对的判据；否则直接省略。
10. 写文件任务：不要用 must_include 猜测文件正文里的具体词，也不要把“最终答复复述文件内容”当判据；只写 criterion/check，让 judge 核对内核附上的文件内容与 tool.fs.write 成功记录。
11. 只输出 JSON，不要解释、不要 markdown 代码围栏。"#;

/// 拼 guide 的用户消息；`feedback` 非空时把上次的 schema 错误原样回注。
fn guide_user_prompt(input: &str, environment: &str, feedback: &str) -> String {
    let mut out = format!("[REVIEW-GUIDE]\n任务输入：\n{input}\n\n环境摘要：\n{environment}");
    let file_task = featheragent::util::requests_file_write(input);
    if file_task {
        out.push_str(
            "\n\n[写文件任务]\n这是写文件任务。只写 criterion/check，让 judge 核对内核提供的文件内容与 tool.fs.write 成功记录；不要写 must_include/must_not_include/min_points/pattern/regex，不要猜文件正文的具体词，也不要要求最终答复逐字复述文件。",
        );
    }
    if !feedback.trim().is_empty() {
        let instruction = if file_task {
            "请对照上面的 JSON 结构重写：写文件任务只写 criterion/check，不要写任何硬键。"
        } else {
            "请对照上面的 JSON 结构重写：判据要写在 must_include / must_not_include / min_points / pattern 上，criterion 和 check 只是说明。"
        };
        out.push_str(&format!(
            "\n\n[上一次的判据不合格，必须修正]\n{}\n{}",
            feedback.trim(),
            instruction
        ));
    }
    out
}

/// 判据上限：条数太多会让 4B 模型直接放弃，太少又退化成单点检查。
/// 判据条数：至少 1 条，最多 5 条。rc.3 要求 2–5 条，7B 只产出 1 条合规项时整轮降级，
/// 反而比“有 1 条硬判据”更糟；这里保留上限，下限降到 1。
const RUBRIC_MIN_ITEMS: usize = 1;
const RUBRIC_MAX_ITEMS: usize = 5;
/// 单条字面判据上限：超过就是“把答案写进判据”。
const MUST_INCLUDE_MAX_CHARS: usize = 12;
const PATH_MUST_INCLUDE_MAX_CHARS: usize = 64;

/// N10：路径/文件名（含 `/`、`\` 或带扩展名的点号形式）允许更长；
/// 这不是放宽普通术语，而是把“文件产出”与“答案原句”区分开。
fn path_like(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return false;
    }
    if t.contains('/') || t.contains('\\') {
        return true;
    }
    match t.rsplit_once('.') {
        Some((stem, ext)) => {
            !stem.is_empty()
                && !ext.is_empty()
                && ext.len() <= 8
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
        }
        None => false,
    }
}
const MUST_NOT_INCLUDE_MAX_CHARS: usize = 20;
const PATTERN_MAX_CHARS: usize = 120;
/// 正则判据上限：太长的正则会拖慢 hardcheck，也不好审。
const REGEX_MAX_CHARS: usize = 200;

/// 把 `must_not_include` 的 string/array 两种写法都收敛成列表。
fn literal_list(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) => vec![s.trim().to_string()],
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// 概念性连接词/系词：must_include 是术语判据，不应写自然语言描述。
fn include_connective(t: &str) -> Option<char> {
    t.chars()
        .find(|c| matches!(c, '和' | '与' | '及' | '或' | '是' | '的' | '了'))
}

/// 把 `must_include` 拆成需要同时出现的词项：空白、中文顿号/逗号/分号；
/// 不含路径点号时也拆 `/`，这样 “Windows/Linux” 不会要求整串连续出现。
/// 这不是汉语分词，只做轻量词项匹配，避免“硬件与软件资源”这类措辞差异误杀。
fn include_terms(phrase: &str) -> Vec<String> {
    let mut split = phrase.to_string();
    if !phrase.contains('.') {
        split = split.replace('/', "\u{0}");
    }
    split
        .split(|c: char| c.is_whitespace() || matches!(c, '、' | '，' | ',' | '；' | ';' | '\u{0}'))
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

/// `must_not_include` 里的 fixture 词/占位符：整条判据自我指涉，直接丢弃。
fn fixture_not_include(t: &str) -> bool {
    let t = t.trim();
    t == "REVIEW-FINAL"
        || t == "REVIEW-START"
        || t.contains("<...>")
        || t.contains("__FILL")
        || t.contains("{{")
        || t.contains("}}")
}

/// 兼容旧调用：没有任务输入时只做结构校验，不检查短语来源。
#[cfg(test)]
fn parse_rubric(text: &str) -> Result<Value, String> {
    parse_rubric_with_input(text, "")
}

/// 示例占位符的形态：尖括号模板、schema 里的空话，或模型直接把键名当值。
fn rubric_placeholder(s: &str) -> bool {
    let t = s.trim();
    t.is_empty()
        || (t.starts_with('<') && t.ends_with('>'))
        || (t.starts_with("__FILL") && t.ends_with("__"))
        || t.contains("{{")
        || t.contains("}}")
        || matches!(
            t,
            "这项检查什么"
                | "怎么核对"
                | "检查项"
                | "核对方式"
                | "criterion"
                | "check"
                | "..."
                | "…"
                | "这条判据检查的任务事实"
                | "怎样从答案核对"
                | "任务要求中必须出现的短语"
                | "禁止项说明"
                | "禁止哪些占位词"
                | "结构要求说明"
                | "至少几个要点、什么格式"
                | "需要 judge 判断的方面"
                | "读答案判断什么"
                | "至少几个要点，填数字"
        )
        || t.contains("这项检查什么")
        || t.contains("怎么核对")
}

/// 判据在讨论占位符/示例本身，而不是当前任务。只有同时出现“占位/示例”类元词
/// 和“禁止/检查/出现”类动作时才命中，避免误杀“解释模板方法”这类真实任务。
fn placeholder_meta(t: &str) -> bool {
    let t = t.trim();
    let meta = [
        "示例中的占位",
        "示例里的占位",
        "示例判据",
        "模板标记",
        "示例模板",
        "__FILL",
        "<...>",
    ];
    let intent = [
        "禁止", "不得", "不能", "不要", "检查", "出现", "使用", "包含", "提到", "填写",
    ];
    meta.iter().any(|m| t.contains(m)) && intent.iter().any(|i| t.contains(i))
}

/// M3：主观评价没有可核对标准，7B judge 每次判都可能不同。
fn subjective_reason(t: &str) -> Option<&'static str> {
    const WORDS: &[&str] = &[
        "简洁",
        "清晰",
        "美观",
        "通顺",
        "易懂",
        "可读性",
        "逻辑性",
        "完整性",
        "语气",
        "排版",
        "准确",
        "准确无误",
        "正确",
        "恰当",
        "专业",
        "规范",
        "流畅",
        "自然",
    ];
    WORDS.iter().find(|w| t.contains(**w)).copied()
}

/// 丢弃把占位符/示例当内容的判据；保留其余条目。返回清洗后的 JSON、丢弃说明、是否全丢。
fn strip_placeholder_meta_items(text: &str) -> (String, Vec<String>, bool) {
    let Ok(mut v) = serde_json::from_str::<Value>(json_slice(text)) else {
        return (text.to_string(), Vec::new(), false);
    };
    let arr = v
        .get("rubric")
        .or_else(|| v.get("items"))
        .and_then(|x| x.as_array())
        .or_else(|| v.as_array())
        .cloned();
    let Some(arr) = arr else {
        return (text.to_string(), Vec::new(), false);
    };
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    for (i, item) in arr.iter().enumerate() {
        let id = item
            .get("id")
            .and_then(|x| x.as_str())
            .filter(|x| !x.trim().is_empty())
            .map(|x| x.to_string())
            .unwrap_or_else(|| format!("c{}", i + 1));
        let criterion = item.get("criterion").and_then(|x| x.as_str()).unwrap_or("");
        let check = item.get("check").and_then(|x| x.as_str()).unwrap_or("");
        // 空 criterion/check 是结构错误，必须交给正式解析报错，不能静默丢弃。
        let fixture_not = item
            .get("must_not_include")
            .map(|v| literal_list(v).iter().any(|b| fixture_not_include(b)))
            .unwrap_or(false);
        let junk = fixture_not
            || (!criterion.trim().is_empty()
                && (rubric_placeholder(criterion) || placeholder_meta(criterion)))
            || (!check.trim().is_empty() && (rubric_placeholder(check) || placeholder_meta(check)));
        if junk {
            dropped.push(format!("{id}（{criterion}）"));
        } else {
            kept.push(item.clone());
        }
    }
    if dropped.is_empty() {
        return (text.to_string(), dropped, false);
    }
    if kept.is_empty() {
        return (text.to_string(), dropped, true);
    }
    if v.get("rubric").and_then(|x| x.as_array()).is_some() {
        v["rubric"] = json!(kept);
    } else if v.get("items").and_then(|x| x.as_array()).is_some() {
        v["items"] = json!(kept);
    } else {
        v = Value::Array(kept);
    }
    (
        serde_json::to_string(&v).unwrap_or_else(|_| text.to_string()),
        dropped,
        false,
    )
}

fn parse_rubric_with_input(text: &str, input: &str) -> Result<Value, String> {
    let (text, dropped, all_dropped) = strip_placeholder_meta_items(text);
    if all_dropped {
        return Err(format!(
            "验收清单全是占位符/示例判据：{}。判据必须描述当前任务，不能讨论占位符或示例本身",
            dropped.join("；")
        ));
    }
    let v: Value = serde_json::from_str(json_slice(&text))
        .map_err(|e| format!("验收清单不是合法 JSON: {e}"))?;
    let arr = v
        .get("rubric")
        .or_else(|| v.get("items"))
        .and_then(|x| x.as_array())
        .or_else(|| v.as_array())
        .ok_or("验收清单缺少 rubric 数组")?;
    if !(RUBRIC_MIN_ITEMS..=RUBRIC_MAX_ITEMS).contains(&arr.len()) {
        return Err(format!(
            "验收清单条数必须在 {RUBRIC_MIN_ITEMS}–{RUBRIC_MAX_ITEMS} 之间，实际 {}",
            arr.len()
        ));
    }
    // N24：明确的写文件任务由 judge 核对内核提供的文件内容；guide 猜出来的
    // must_include 不能拿来卡“最终答复”。这里把确定性键收走，统一转描述型。
    let file_task = featheragent::util::requests_file_write(input);
    let mut out = Vec::new();
    let mut hard_items = 0usize;
    let mut substring_only = true;
    let mut judge_only_ids = Vec::new();
    let mut advisory_ids = Vec::new();
    let mut advisory_warnings = Vec::new();
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
        if rubric_placeholder(criterion) || rubric_placeholder(check) {
            return Err(format!(
                "验收项 {} 的 criterion/check 是示例占位符或空话（criterion=『{criterion}』，check=『{check}』）：必须换成当前任务的具体判据",
                i + 1
            ));
        }
        // N29：写文件任务不按答案字面校验 guide 产出的硬键。文件任务的判据天然
        // 会写“行首 - ”这类符号（会被 2 字术语下限误杀），也可能与“必须有硬键”
        // 的普通任务规则冲突。这里一次性剥掉所有硬键，只留 criterion/check 给
        // judge 结合文件正文核对；空/占位 criterion 仍会被上面的检查拦住。
        if file_task {
            let id = item
                .get("id")
                .and_then(|x| x.as_str())
                .filter(|x| !x.trim().is_empty())
                .map(|x| x.to_string())
                .unwrap_or_else(|| format!("c{}", i + 1));
            let weight = item.get("weight").and_then(|x| x.as_f64()).unwrap_or(1.0);
            let mut norm = serde_json::Map::new();
            norm.insert("id".into(), json!(id));
            norm.insert("criterion".into(), json!(criterion));
            norm.insert("check".into(), json!(check));
            norm.insert("weight".into(), json!(weight.clamp(0.1, 10.0)));
            norm.insert("judge_only".into(), json!(true));
            norm.insert("file_task".into(), json!(true));
            judge_only_ids.push(id.clone());
            out.push(Value::Object(norm));
            continue;
        }
        let mut norm = serde_json::Map::new();
        // must_include：关键词/短语，禁整句，≤20 字。7B 有时写成数组（如 ["SYN","ACK"]），
        // 这里把 string / array 两种写法都收敛成列表，由 hardcheck 逐项核对。
        let mut must_include_values: Vec<String> =
            literal_list(item.get("must_include").unwrap_or(&Value::Null))
                .into_iter()
                .map(|x| {
                    x.trim_matches(|c| {
                        matches!(c, '『' | '』' | '「' | '」' | '`' | '"' | '“' | '”')
                    })
                    .trim()
                    .to_string()
                })
                .filter(|x| !x.is_empty())
                .collect();
        if must_include_values.is_empty() {
            if let Some(lit) = {
                // 只有在模型没写任何判据时才从 check 里提取『…』；否则 must_not_include 里的
                // 被禁词会被误当成 must_include，等于把禁词变成必选词。
                let has_explicit = item
                    .get("must_not_include")
                    .map(|v| !literal_list(v).is_empty())
                    .unwrap_or(false)
                    || item.get("min_points").is_some()
                    || item
                        .get("pattern")
                        .and_then(|x| x.as_str())
                        .map(|x| !x.trim().is_empty())
                        .unwrap_or(false)
                    || item
                        .get("regex")
                        .and_then(|x| x.as_str())
                        .map(|x| !x.trim().is_empty())
                        .unwrap_or(false)
                    || item.get("hard").is_some();
                if has_explicit {
                    None
                } else {
                    quoted_literal(check)
                }
            } {
                must_include_values.push(lit);
            }
        }
        for lit in &must_include_values {
            // N10：文件名/路径不是“答案原句”，12 字上限会把 workspace/notes.txt 误伤。
            // 路径/文件名放宽到 64 字；普通术语仍保持 12 字。
            let limit = if path_like(lit) {
                PATH_MUST_INCLUDE_MAX_CHARS
            } else {
                MUST_INCLUDE_MAX_CHARS
            };
            if lit.chars().count() > limit {
                let hint = if path_like(lit) {
                    "路径/文件名最多 64 字；如果真正要核对的是文件产出，建议改用 pattern/regex 或 criterion/check"
                } else {
                    "判据只能是短语，不能是答案原句"
                };
                return Err(format!(
                    "验收项 {} 的 must_include 超过 {limit} 字（{} 字）：{hint}",
                    i + 1,
                    lit.chars().count()
                ));
            }
            if sentence_like(lit) {
                return Err(format!(
                    "验收项 {} 的 must_include 是整句：判据只能是关键词/短语",
                    i + 1
                ));
            }
            if rubric_placeholder(lit) {
                return Err(format!(
                    "验收项 {} 的 must_include 是占位符『{lit}』：必须换成当前任务里必须出现的具体短语",
                    i + 1
                ));
            }
            let n_chars = lit.chars().count();
            if n_chars < 2 {
                return Err(format!(
                    "验收项 {} 的 must_include『{lit}』只有 {n_chars} 字：术语判据至少 2 字",
                    i + 1
                ));
            }
            if let Some(c) = include_connective(lit) {
                return Err(format!(
                    "验收项 {} 的 must_include『{lit}』含连接词『{c}』：这是概念性描述，不是术语。术语判据只写专有名词/固定写法；概念要求请写在 criterion/check 交给 judge",
                    i + 1
                ));
            }
            // L1：REVIEW-FINAL/REVIEW-START 是 mock 夹具的测试标记，7B 容易从示例里抄来。
            // 用户任务里明确出现时才允许；否则正常答案永远无法满足。
            // 只看报告里出现的两个 mock 标记；不要扩大到所有大写词，
            // 否则 “HTTP”“JSON” 这类任务内本来就该出现的术语会被误杀。
            let test_marker = matches!(lit.as_str(), "REVIEW-FINAL" | "REVIEW-START");
            // 集成测试的输入统一是 REVIEW_* 夹具令牌；生产任务不会长这样。
            let fixture_mode = input.contains("REVIEW_START")
                || input.contains("REVIEW-FINAL")
                || input.trim_start().starts_with("REVIEW_");
            if !input.trim().is_empty()
                && test_marker
                && !fixture_mode
                && !input.contains(lit.as_str())
            {
                return Err(format!(
                    "验收项 {} 的 must_include『{lit}』是示例/测试标记，且没有出现在用户任务输入中：判据必须来自当前任务",
                    i + 1
                ));
            }
        }
        let must_count = must_include_values.len();
        let match_mode = item
            .get("match")
            .and_then(|x| x.as_str())
            .map(|x| x.trim().to_lowercase())
            .filter(|x| !x.is_empty())
            .unwrap_or_else(|| {
                if must_count > 1 {
                    "min".into()
                } else {
                    "all".into()
                }
            });
        if must_count == 0 && item.get("match").is_some() {
            return Err(format!(
                "验收项 {} 写了 match，但 must_include 为空；match 只用于多项 must_include",
                i + 1
            ));
        }
        let include_min = if must_count == 0 {
            0
        } else {
            match match_mode.as_str() {
                "all" => must_count,
                "any" => 1,
                "min" => item
                    .get("min_matches")
                    .and_then(featheragent::util::json_u64)
                    .map(|n| n as usize)
                    .unwrap_or(must_count.div_ceil(2)),
                other => {
                    return Err(format!(
                        "验收项 {} 的 match 只能是 any/all/min，实际『{other}』",
                        i + 1
                    ))
                }
            }
            .clamp(1, must_count)
        };
        if must_count > 0 && item.get("min_matches").is_some() {
            let n = item
                .get("min_matches")
                .and_then(featheragent::util::json_u64)
                .unwrap_or(0) as usize;
            if n < 1 || n > must_count {
                return Err(format!(
                    "验收项 {} 的 min_matches 必须在 1–{must_count} 之间（must_include 共 {must_count} 项）",
                    i + 1
                ));
            }
        }
        if !must_include_values.is_empty() {
            let value = if must_include_values.len() == 1 {
                json!(must_include_values[0])
            } else {
                json!(must_include_values)
            };
            norm.insert("must_include".into(), value);
            norm.insert("match".into(), json!(match_mode));
            norm.insert("min_matches".into(), json!(include_min));
        }
        let banned = literal_list(item.get("must_not_include").unwrap_or(&Value::Null));
        if !banned.is_empty() {
            // N6：主观判据（如“术语准确”）不能用禁掉常用词的 must_not_include 来实现；
            // 这样会把“软件/硬件/计算机”这类答案必用词判死，失败原因还与判据无关。
            if let Some(word) = subjective_reason(criterion).or_else(|| subjective_reason(check)) {
                return Err(format!(
                    "验收项 {} 是主观判据（含『{word}』），不能用 must_not_include 实现；主观要求请只写 criterion/check 交给 judge，或改成可计数/可核对的 format",
                    i + 1
                ));
            }
            // 禁词已经出现在用户任务输入里：判据和任务自相矛盾，要求 guide 重写。
            let input_low = input.to_lowercase();
            if let Some(b) = banned
                .iter()
                .find(|b| !b.trim().is_empty() && input_low.contains(&b.to_lowercase()))
            {
                return Err(format!(
                    "验收项 {} 的 must_not_include『{b}』出现在用户任务输入中：禁词与任务矛盾，请改成当前任务真正应避免的内容",
                    i + 1
                ));
            }
            if let Some(b) = banned.iter().find(|b| rubric_placeholder(b)) {
                return Err(format!(
                    "验收项 {} 的 must_not_include 含占位符『{b}』：必须换成具体的禁词",
                    i + 1
                ));
            }
            if banned
                .iter()
                .any(|b| b.chars().count() > MUST_NOT_INCLUDE_MAX_CHARS)
            {
                return Err(format!(
                    "验收项 {} 的 must_not_include 超过 {MUST_NOT_INCLUDE_MAX_CHARS} 字",
                    i + 1
                ));
            }
            substring_only = false;
            norm.insert("must_not_include".into(), json!(banned));
        }
        if let Some(n) = item.get("min_points").and_then(|x| x.as_u64()) {
            if !(1..=12).contains(&n) {
                return Err(format!("验收项 {} 的 min_points 必须在 1–12 之间", i + 1));
            }
            substring_only = false;
            norm.insert("min_points".into(), json!(n));
        }
        if let Some(pat) = item
            .get("pattern")
            .and_then(|x| x.as_str())
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
        {
            if pat.chars().count() > PATTERN_MAX_CHARS {
                return Err(format!("验收项 {} 的 pattern 过长", i + 1));
            }
            substring_only = false;
            norm.insert("pattern".into(), json!(pat));
        }
        // 真正则判据：`regex` + 可选 `min_matches`（默认 1）。语法错误在 guide 阶段就拒绝，
        // 不要留到 hardcheck 才炸成 degraded。
        if let Some(pat) = item
            .get("regex")
            .and_then(|x| x.as_str())
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
        {
            if pat.chars().count() > REGEX_MAX_CHARS {
                return Err(format!("验收项 {} 的 regex 过长", i + 1));
            }
            regex::Regex::new(&pat)
                .map_err(|e| format!("验收项 {} 的 regex 不是合法正则：{e}", i + 1))?;
            substring_only = false;
            norm.insert("regex".into(), json!(pat));
            if let Some(n) = item.get("min_matches").and_then(|x| x.as_u64()) {
                if !(1..=50).contains(&n) {
                    return Err(format!("验收项 {} 的 min_matches 必须在 1–50 之间", i + 1));
                }
                norm.insert("min_matches".into(), json!(n));
            }
        }
        if item.get("hard").is_some() {
            norm.insert("hard".into(), item.get("hard").cloned().unwrap());
            substring_only = false;
        }
        let id = item
            .get("id")
            .and_then(|x| x.as_str())
            .filter(|x| !x.trim().is_empty())
            .map(|x| x.to_string())
            .unwrap_or_else(|| format!("c{}", i + 1));
        // 只有描述、没有硬判据的条目不当废品：保留给 judge 逐条核对，
        // 由 `review.warn` 明示“这条没有确定性检查”，而不是整轮 degraded。
        if file_task {
            for key in [
                "must_include",
                "must_not_include",
                "min_points",
                "pattern",
                "regex",
                "hard",
                "match",
                "min_matches",
            ] {
                norm.remove(key);
            }
            judge_only_ids.push(id.clone());
            norm.insert("judge_only".into(), json!(true));
            norm.insert("file_task".into(), json!(true));
        } else {
            let has_hard = norm.keys().any(|k| {
                matches!(
                    k.as_str(),
                    "must_include"
                        | "must_not_include"
                        | "min_points"
                        | "pattern"
                        | "regex"
                        | "hard"
                )
            });
            if has_hard {
                hard_items += 1;
            } else if let Some(word) =
                subjective_reason(criterion).or_else(|| subjective_reason(check))
            {
                // M3：主观判据不进 judge，也不计入门禁；只留一条 review.warn。
                advisory_ids.push(id.clone());
                advisory_warnings.push(format!(
                    "验收项 {id} 是主观判据（含『{word}』）：不计入门禁；需要时改成可计数/可核对的格式"
                ));
                norm.insert("advisory".into(), json!(true));
                norm.insert("advisory_reason".into(), json!(word));
            } else {
                judge_only_ids.push(id.clone());
                norm.insert("judge_only".into(), json!(true));
            }
        }
        let weight = item.get("weight").and_then(|x| x.as_f64()).unwrap_or(1.0);
        norm.insert("id".into(), json!(id));
        norm.insert("criterion".into(), json!(criterion));
        norm.insert("check".into(), json!(check));
        norm.insert("weight".into(), json!(weight.clamp(0.1, 10.0)));
        out.push(Value::Object(norm));
    }
    // 没有硬判据不直接判死：内核会带错误回注再要一次，仍拿不到才降级到 judge-only。
    // 这样 7B 不会因为“键名写错”整轮 degraded（rc.3 的 I1），同时错误信息足够具体。
    let mut warnings = advisory_warnings;
    for d in &dropped {
        warnings.push(format!(
            "已丢弃占位符/示例判据 {d}：判据要描述当前任务，不能讨论占位符或示例本身"
        ));
    }
    if file_task {
        warnings.push(
            "写文件任务：判据已转为 judge 核对文件内容与 tool.fs.write 成功记录；不按最终答复的字面措辞判定"
                .to_string(),
        );
    } else if hard_items == 0 {
        warnings.push(
            "整份清单没有硬判据：criterion/check 只是说明，判据必须写在 must_include / must_not_include / min_points / pattern / regex 上"
                .to_string(),
        );
    }
    if !judge_only_ids.is_empty() {
        warnings.push(format!(
            "{} 条描述型判据（{}）：由 judge 按 criterion 评估",
            judge_only_ids.len(),
            judge_only_ids.join(", ")
        ));
    } else {
        warnings.push("没有描述型判据：judge 不参与本轮，全部由确定性判据决定".to_string());
    }
    if substring_only && !file_task {
        warnings.push(
            "判据全是字面短语，建议再加一条 must_not_include / min_points / pattern 以覆盖格式与要点"
                .to_string(),
        );
    }
    Ok(json!({
        "rubric": out,
        "count": out.len(),
        "hard_items": hard_items,
        "judge_only": judge_only_ids,
        "advisory": advisory_ids,
        "file_task": file_task,
        "warn": warnings,
    }))
}

fn parse_verdict(text: &str, rubric: &Value, answer: &str) -> Result<Value, String> {
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
        // evidence 必须能在答案里找到连续原文：抄提示词、写空话都不算证据。
        if !evidence_quotes(answer, &evidence) {
            return Err(format!(
                "审批项 {id} 的 evidence 不是答案原文（须为答案中连续 ≥10 字）：{evidence:?}"
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
    if !evidence_quotes(answer, summary) {
        return Err(format!(
            "审批总结 evidence 不是答案原文（须为答案中连续 ≥10 字）：{summary:?}"
        ));
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
        let ok = parse_rubric(
            "```json\n{\"rubric\":[
                {\"id\":\"c1\",\"criterion\":\"包含结论\",\"check\":\"答案第一段有『REVIEW-FINAL』\",\"must_include\":\"REVIEW-FINAL\"},
                {\"id\":\"c2\",\"criterion\":\"不得出现占位词\",\"check\":\"禁止『TODO』\",\"must_not_include\":\"TODO\"}
            ]}\n```",
        )
        .unwrap();
        assert_eq!(ok["count"], 2);
        assert_eq!(ok["rubric"][0]["id"], "c1");
        assert_eq!(ok["rubric"][0]["must_include"], "REVIEW-FINAL");
        let quoted = parse_rubric(
            "{\"rubric\":[
                {\"id\":\"c1\",\"criterion\":\"包含结论\",\"check\":\"答案出现『REVIEW-FINAL』\"},
                {\"id\":\"c2\",\"criterion\":\"至少两个要点\",\"check\":\"分点作答\",\"min_points\":2}
            ]}",
        )
        .unwrap();
        assert_eq!(quoted["rubric"][0]["must_include"], "REVIEW-FINAL");
        let bad = parse_rubric(
            "{\"rubric\":[{\"criterion\":\"只有标题\",\"check\":\"\"},{\"criterion\":\"x\",\"check\":\"y\"}]}",
        );
        assert!(bad.is_err());
        // rc.3.1：单项不再直接判死（7B 常常只给一条合规项），但要打 warn。
        let single = parse_rubric(
            "{\"rubric\":[{\"id\":\"c1\",\"criterion\":\"包含结论\",\"check\":\"有『REVIEW-FINAL』\",\"must_include\":\"REVIEW-FINAL\"}]}",
        )
        .unwrap();
        assert_eq!(single["count"], 1);
        assert_eq!(single["hard_items"], 1);
        // 全是子串判据：保留但告警，提示补一条格式/要点判据。
        let only_substring = parse_rubric(
            "{\"rubric\":[
                {\"id\":\"c1\",\"criterion\":\"a\",\"check\":\"有『REVIEW-FINAL』\",\"must_include\":\"REVIEW-FINAL\"},
                {\"id\":\"c2\",\"criterion\":\"b\",\"check\":\"有『w1.txt』\",\"must_include\":\"w1.txt\"}
            ]}",
        )
        .unwrap();
        assert_eq!(only_substring["hard_items"], 2);
        assert!(only_substring["warn"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap_or("").contains("字面")));
    }

    #[test]
    fn review_rubric_accepts_description_only_items_with_warning() {
        // rc.3 真机 I1：7B 只产出 criterion/check 时不再整轮 degraded，
        // 条目降级为 judge-only，并把“没有硬检查”作为 review.warn 抛给前端。
        let soft = parse_rubric(
            "{\"rubric\":[
                {\"id\":\"c1\",\"criterion\":\"答案要说明文件不存在\",\"check\":\"读答案判断\"},
                {\"id\":\"c2\",\"criterion\":\"给出失败原因\",\"check\":\"读答案判断\"}
            ]}",
        )
        .unwrap();
        assert_eq!(soft["count"], 2);
        assert_eq!(soft["hard_items"], 0);
        assert_eq!(soft["rubric"][0]["judge_only"], true);
        assert_eq!(soft["judge_only"][0], "c1");
        assert!(soft["warn"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap_or("").contains("没有硬判据")));
    }

    #[test]
    fn guide_prompt_injects_previous_schema_error() {
        let first = guide_user_prompt("任务", "环境", "");
        assert!(!first.contains("上一次"), "首次不应带反馈");
        let retry = guide_user_prompt("任务", "环境", "验收项 1 没有判据");
        assert!(retry.contains("上一次的判据不合格"));
        assert!(retry.contains("验收项 1 没有判据"));
        assert!(GUIDE_SYSTEM.contains("must_include"));
        assert!(GUIDE_SYSTEM.contains("1–5 条"));
    }

    #[test]
    fn review_rubric_rejects_whole_sentence_must_include() {
        let sentence = "操作系统是管理计算机硬件和软件资源的系统。";
        let err = parse_rubric(&format!(
            "{{\"rubric\":[
                {{\"id\":\"c1\",\"criterion\":\"结论\",\"check\":\"出现结论句\",\"must_include\":\"{sentence}\"}},
                {{\"id\":\"c2\",\"criterion\":\"禁词\",\"check\":\"禁止『TODO』\",\"must_not_include\":\"TODO\"}}
            ]}}"
        ))
        .unwrap_err();
        assert!(err.contains("整句") || err.contains("12"), "{err}");
        let long = parse_rubric(
            "{\"rubric\":[
                {\"id\":\"c1\",\"criterion\":\"结论\",\"check\":\"出现关键词\",\"must_include\":\"REVIEW-FINAL-PLACEHOLDER-TOKEN-VERY-LONG\"},
                {\"id\":\"c2\",\"criterion\":\"禁词\",\"check\":\"禁止『TODO』\",\"must_not_include\":\"TODO\"}
            ]}",
        )
        .unwrap_err();
        assert!(long.contains("12"), "{long}");
    }

    #[test]
    fn review_rubric_allows_file_path_must_include() {
        let v = parse_rubric(
            "{\"rubric\":[
                {\"id\":\"c1\",\"criterion\":\"写入 notes.txt\",\"check\":\"文件存在\",\"must_include\":[\"workspace/notes.txt\"]}
            ]}",
        )
        .unwrap();
        assert_eq!(v["rubric"][0]["must_include"], "workspace/notes.txt");
        assert_eq!(v["rubric"][0]["match"], "all");
        let long_path = format!("{}/{}", "a".repeat(60), "notes.txt");
        let err = parse_rubric(&format!(
            "{{\"rubric\":[{{\"id\":\"c1\",\"criterion\":\"写入文件\",\"check\":\"文件存在\",\"must_include\":[\"{long_path}\"]}}]}}"
        ))
        .unwrap_err();
        assert!(err.contains("64"), "{err}");
        assert!(err.contains("pattern/regex"), "{err}");
    }

    #[test]
    fn review_rubric_accepts_new_criterion_kinds() {
        let v = parse_rubric(
            "{\"rubric\":[
                {\"id\":\"c1\",\"criterion\":\"有结论\",\"check\":\"出现『REVIEW-FINAL』\",\"must_include\":\"REVIEW-FINAL\"},
                {\"id\":\"c2\",\"criterion\":\"分点\",\"check\":\"至少三个要点\",\"min_points\":3},
                {\"id\":\"c3\",\"criterion\":\"格式\",\"check\":\"匹配 workspace/*.txt\",\"pattern\":\"workspace/*.txt\"}
            ]}",
        )
        .unwrap();
        assert_eq!(v["rubric"][1]["min_points"], 3);
        assert_eq!(v["rubric"][2]["pattern"], "workspace/*.txt");
    }

    #[test]
    fn review_verdict_short_or_prompt_echo_evidence_is_rejected() {
        let answer = "第一，操作系统管理硬件与软件资源；第二，它提供用户界面和程序接口。";
        let rubric = json!([
            {"id":"c1","criterion":"包含结论","check":"答案出现『硬件』"},
            {"id":"c2","criterion":"包含要点","check":"答案出现『用户界面』"}
        ]);
        let short = parse_verdict(
            "{\"pass\":true,\"items\":[{\"id\":\"c1\",\"pass\":true,\"evidence\":\"总结\"},{\"id\":\"c2\",\"pass\":true,\"evidence\":\"总结\"}],\"evidence\":\"总结\"}",
            &rubric,
            answer,
        );
        assert!(short.is_err());
        // 抄提示词：长度够但不是答案原文，必须拒绝。
        let echo = parse_verdict(
            "{\"pass\":true,\"items\":[{\"id\":\"c1\",\"pass\":true,\"evidence\":\"引用答案中的原文至少 10 字，禁止总结通过 ok 是等占位词\"},{\"id\":\"c2\",\"pass\":true,\"evidence\":\"每条 evidence 必须引用答案原文并至少 10 字\"}],\"evidence\":\"逐条核对结论，至少 10 字，禁止占位词\"}",
            &rubric,
            answer,
        );
        assert!(echo.is_err());
        let ok = parse_verdict(
            "{\"pass\":true,\"items\":[{\"id\":\"c1\",\"pass\":true,\"evidence\":\"操作系统管理硬件与软件资源\"},{\"id\":\"c2\",\"pass\":true,\"evidence\":\"它提供用户界面和程序接口\"}],\"evidence\":\"第一，操作系统管理硬件与软件资源\"}",
            &rubric,
            answer,
        )
        .unwrap();
        assert_eq!(ok["pass"], true);
        assert_eq!(ok["items"][0]["criterion"], "包含结论");
    }

    #[test]
    fn review_rubric_accepts_regex_and_rejects_invalid_one() {
        let v = parse_rubric(
            "{\"rubric\":[
                {\"id\":\"c1\",\"criterion\":\"编号\",\"check\":\"至少两个数字编号\",\"regex\":\"(?m)^\\\\d+\\\\. \",\"min_matches\":2},
                {\"id\":\"c2\",\"criterion\":\"用途\",\"check\":\"读答案判断\"}
            ]}",
        )
        .unwrap();
        assert_eq!(v["rubric"][0]["min_matches"], 2);
        assert_eq!(v["judge_only"].as_array().unwrap().len(), 1);
        let err = parse_rubric(
            "{\"rubric\":[
                {\"id\":\"c1\",\"criterion\":\"坏正则\",\"check\":\"x\",\"regex\":\"(unclosed\"}
            ]}",
        )
        .unwrap_err();
        assert!(err.contains("regex") || err.contains("正则"), "{err}");
    }

    #[test]
    fn review_rubric_rejects_schema_placeholders() {
        let err = parse_rubric_with_input(
            "{\"rubric\":[
                {\"id\":\"c1\",\"criterion\":\"这项检查什么\",\"check\":\"怎么核对\",\"must_include\":\"REVIEW-FINAL\"}
            ]}",
            "用三句话说明什么是操作系统",
        )
        .unwrap_err();
        assert!(err.contains("占位符") || err.contains("示例"), "{err}");
        let err = parse_rubric_with_input(
            "{\"rubric\":[
                {\"id\":\"c1\",\"criterion\":\"这条判据检查的任务事实\",\"check\":\"怎样从答案核对\",\"must_include\":\"操作系统\"}
            ]}",
            "用三句话说明什么是操作系统",
        )
        .unwrap_err();
        assert!(err.contains("占位符") || err.contains("示例"), "{err}");
        let err = parse_rubric_with_input(
            "{\"rubric\":[
                {\"id\":\"c1\",\"criterion\":\"结论\",\"check\":\"出现关键词\",\"must_include\":\"REVIEW-FINAL\"}
            ]}",
            "用三句话说明什么是操作系统",
        )
        .unwrap_err();
        assert!(err.contains("测试标记") || err.contains("任务"), "{err}");
        let ok = parse_rubric_with_input(
            "{\"rubric\":[
                {\"id\":\"c1\",\"criterion\":\"答案说明了操作系统是什么\",\"check\":\"出现『操作系统』\",\"must_include\":\"操作系统\"},
                {\"id\":\"c2\",\"criterion\":\"没有跑题\",\"check\":\"读答案判断\"}
            ]}",
            "用三句话说明什么是操作系统",
        )
        .unwrap();
        assert_eq!(ok["hard_items"], 1);
    }

    #[test]
    fn review_rubric_drops_placeholder_meta_item_and_keeps_task_items() {
        let v = parse_rubric_with_input(
            "{\"rubric\":[
                {\"id\":\"c1\",\"criterion\":\"说明 TCP 三次握手的过程\",\"check\":\"检查答案是否包含三个阶段\",\"must_include\":[\"SYN\",\"ACK\"]},
                {\"id\":\"c2\",\"criterion\":\"禁止使用示例中的占位词\",\"check\":\"检查答案中是否出现 <...> 占位符\",\"must_not_include\":[\"<...>\"]},
                {\"id\":\"c3\",\"criterion\":\"至少包含三个要点\",\"check\":\"用分号或列表分点\",\"min_points\":3}
            ]}",
            "用三句话说明 TCP 三次握手，必须提到 SYN 和 ACK",
        )
        .unwrap();
        assert_eq!(v["count"], 2, "{v}");
        assert_eq!(v["rubric"][0]["id"], "c1");
        assert_eq!(v["rubric"][1]["id"], "c3");
        assert_eq!(v["hard_items"], 2);
        assert!(
            v["warn"].as_array().unwrap().iter().any(|w| w
                .as_str()
                .unwrap_or("")
                .contains("已丢弃")
                && w.as_str().unwrap_or("").contains("c2")),
            "{v}"
        );
    }

    #[test]
    fn review_rubric_marks_subjective_item_advisory() {
        let v = parse_rubric_with_input(
            "{\"rubric\":[
                {\"id\":\"c1\",\"criterion\":\"说明操作系统的作用\",\"check\":\"出现『操作系统』\",\"must_include\":\"操作系统\"},
                {\"id\":\"c4\",\"criterion\":\"判断答案是否简洁明了\",\"check\":\"读答案判断内容是否清晰、简洁\"}
            ]}",
            "用三句话说明什么是操作系统",
        )
        .unwrap();
        assert_eq!(v["hard_items"], 1);
        assert_eq!(v["rubric"][1]["advisory"], true);
        assert!(v["judge_only"].as_array().unwrap().is_empty(), "{v}");
        assert!(
            v["warn"]
                .as_array()
                .unwrap()
                .iter()
                .any(|w| w.as_str().unwrap_or("").contains("主观判据")),
            "{v}"
        );
    }

    #[test]
    fn review_rubric_accepts_array_must_include_and_hardcheck_requires_all() {
        let v = parse_rubric_with_input(
            "{\"rubric\":[
                {\"id\":\"c1\",\"criterion\":\"说明 TCP 三次握手的过程\",\"check\":\"包含 SYN 和 ACK\",\"match\":\"all\",\"must_include\":[\"SYN\",\"ACK\"]},
                {\"id\":\"c2\",\"criterion\":\"至少三个要点\",\"check\":\"分点说明\",\"min_points\":3}
            ]}",
            "用三句话说明 TCP 三次握手，必须提到 SYN 和 ACK",
        )
        .unwrap();
        assert_eq!(v["hard_items"], 2, "{v}");
        assert_eq!(v["rubric"][0]["must_include"], json!(["SYN", "ACK"]));
        let bad = review_gate_plugin(
            "review.hardcheck",
            &json!({"answer":"客户端发送 SYN，等待服务端响应。\n1. 第一\n2. 第二", "rubric":v["rubric"]}),
        )
        .unwrap();
        assert_eq!(bad["pass"], false, "{bad}");
        assert_eq!(bad["failed"][0]["id"], "c1", "{bad}");
        let ok = review_gate_plugin(
            "review.hardcheck",
            &json!({"answer":"客户端发送 SYN，服务端回复 SYN-ACK，最后客户端发送 ACK。\n1. 建立连接\n2. 交换序号\n3. 确认双向通信", "rubric":v["rubric"]}),
        )
        .unwrap();
        assert_eq!(ok["pass"], true, "{ok}");
    }

    #[test]
    fn review_hardcheck_does_not_leak_must_include_literal() {
        let out = review_gate_plugin(
            "review.hardcheck",
            &json!({
                "answer": "操作系统是管理硬件与软件资源的系统。",
                "rubric": [{
                    "id":"c1",
                    "criterion":"答案提到操作系统",
                    "check":"出现关键词",
                    "must_include":"REVIEW-FINAL"
                }]
            }),
        )
        .unwrap();
        let evidence = out["failed"][0]["evidence"].as_str().unwrap_or("");
        assert!(
            !evidence.contains("REVIEW-FINAL"),
            "不得把字面短语回显给 exec: {evidence}"
        );
        assert!(evidence.contains("未覆盖"), "{evidence}");
    }

    #[test]
    fn review_hardcheck_covers_all_criterion_kinds() {
        let rubric = json!([
            {"id":"c1","criterion":"结论","check":"出现『REVIEW-FINAL』","must_include":"REVIEW-FINAL"},
            {"id":"c2","criterion":"禁词","check":"禁止『TODO』","must_not_include":["TODO","FIXME"]},
            {"id":"c3","criterion":"要点","check":"至少两个要点","min_points":2},
            {"id":"c4","criterion":"路径","check":"匹配 workspace/*.txt","pattern":"workspace/*.txt"},
            {"id":"c5","criterion":"编号","check":"至少两个数字编号","regex":"(?m)^\\d+\\. ","min_matches":2}
        ]);
        let bad = review_gate_plugin(
            "review.hardcheck",
            &json!({"answer":"这里只有一段普通总结 TODO", "rubric":rubric}),
        )
        .unwrap();
        assert_eq!(bad["pass"], false);
        let ids: Vec<&str> = bad["failed"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|x| x["id"].as_str())
            .collect();
        assert!(ids.contains(&"c1"));
        assert!(ids.contains(&"c2"));
        let ok = review_gate_plugin(
            "review.hardcheck",
            &json!({"answer":"最终答案 REVIEW-FINAL\n1. 第一点\n2. 第二点\n路径 workspace/a.txt", "rubric":rubric}),
        )
        .unwrap();
        assert_eq!(ok["pass"], true, "{ok}");
    }

    #[test]
    fn glob_and_point_helpers_work() {
        assert!(glob_match("workspace/*.txt", "workspace/a.txt"));
        assert!(glob_match("*.md|*.txt", "notes.md"));
        assert!(!glob_match("workspace/*.txt", "workspace/a.md"));
        assert_eq!(count_points("- a\n* b\n1. c\n2、d\n普通一行"), 4);
        assert!(sentence_like("操作系统是管理系统。"));
        assert!(!sentence_like("REVIEW-FINAL"));
    }

    #[test]
    fn count_points_recognizes_chinese_inline_enumeration() {
        // J1 真机样本：一段里用分号并列三点，旧实现只数出行首列表标记 → 误判 1 个。
        let answer = "操作系统是系统级软件，负责协调硬件与软件资源；它通过进程调度、内存管理和中断处理实现多任务；它还提供文件系统等用户接口。";
        assert!(count_points(answer) >= 3, "分号枚举应算出至少 3 个要点");
        let bold = "**第一点**：资源管理\n**第二点**：接口抽象";
        assert_eq!(count_points(bold), 2);
        assert_eq!(count_points("只有一句话。"), 1);
    }

    #[test]
    fn min_points_is_soft_and_defers_to_judge() {
        let rubric = json!([
            {"id":"c1","criterion":"命中标记","check":"出现『REVIEW-FINAL』","must_include":"REVIEW-FINAL"},
            {"id":"c2","criterion":"至少两个要点","check":"分点作答","min_points":2}
        ]);
        let v = review_gate_plugin(
            "review.hardcheck",
            &json!({"answer":"REVIEW-FINAL", "rubric":rubric}),
        )
        .unwrap();
        assert_eq!(v["pass"], true, "min_points 不足不应硬判死: {v}");
        assert_eq!(v["failed"].as_array().unwrap().len(), 0);
        let soft = v["soft"].as_array().unwrap();
        assert_eq!(soft.len(), 1);
        assert_eq!(soft[0]["id"], "c2");
        assert_eq!(soft[0]["need"], 2);
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
    #[test]
    fn review_rubric_rejects_conceptual_must_include_with_connective() {
        let err = parse_rubric_with_input(
        "{\"rubric\":[{\"id\":\"c1\",\"criterion\":\"说明操作系统\",\"check\":\"覆盖硬件与软件\",\"must_include\":\"管理计算机硬件和资源\"},{\"id\":\"c2\",\"criterion\":\"整体可读\",\"check\":\"读答案判断\"}]}",
        "用三句话说明什么是操作系统",
    )
    .unwrap_err();
        assert!(err.contains("连接词"), "{err}");
        assert!(err.contains("概念性描述"), "{err}");
    }

    #[test]
    fn review_rubric_rejects_one_char_must_include() {
        let err = parse_rubric_with_input(
        "{\"rubric\":[{\"id\":\"c1\",\"criterion\":\"说明操作系统\",\"check\":\"出现关键词\",\"must_include\":\"硬\"},{\"id\":\"c2\",\"criterion\":\"整体可读\",\"check\":\"读答案判断\"}]}",
        "用三句话说明什么是操作系统",
    )
    .unwrap_err();
        assert!(err.contains("至少 2 字"), "{err}");
    }

    #[test]
    fn review_hardcheck_matches_include_terms_across_wording() {
        let out = review_gate_plugin(
        "review.hardcheck",
        &json!({
            "answer": "操作系统是管理计算机硬件与软件资源的系统。",
            "rubric": [{"id":"c1","criterion":"覆盖硬件和软件","check":"两个词都出现","must_include":"管理计算机硬件 资源"}]
        }),
    )
    .unwrap();
        assert_eq!(out["pass"], true, "{out}");
    }

    #[test]
    fn review_rubric_drops_fixture_must_not_include_item() {
        let v = parse_rubric_with_input(
        r#"{"rubric":[
            {"id":"c1","criterion":"说明操作系统","check":"出现『操作系统』","must_include":"操作系统"},
            {"id":"c4","criterion":"避免照搬答案","check":"没有直接复制示例中的句子或段落","must_not_include":["REVIEW-FINAL","REVIEW-START","\u003c...\u003e"]},
            {"id":"c5","criterion":"整体可读","check":"读答案判断"}
        ]}"#,
        "用三句话说明什么是操作系统",
    )
    .unwrap();
        assert_eq!(v["count"], 2, "{v}");
        assert!(
            v["warn"].as_array().unwrap().iter().any(|w| w
                .as_str()
                .unwrap_or("")
                .contains("已丢弃")
                && w.as_str().unwrap_or("").contains("c4")),
            "{v}"
        );
    }

    #[test]
    fn review_rubric_defaults_multi_must_include_to_half() {
        let v = parse_rubric_with_input(
            r#"{"rubric":[
                {"id":"c1","criterion":"说明文件与进程管理","check":"覆盖功能","must_include":["文件管理","进程管理","设备驱动","用户界面"]},
                {"id":"c2","criterion":"整体可读","check":"读答案判断"}
            ]}"#,
            "用三句话说明什么是操作系统",
        )
        .unwrap();
        assert_eq!(v["rubric"][0]["match"], "min", "{v}");
        assert_eq!(v["rubric"][0]["min_matches"], 2, "{v}");
        let pass = review_gate_plugin(
            "review.hardcheck",
            &json!({"answer":"操作系统负责进程管理、内存分配，也提供用户界面。", "rubric":v["rubric"]}),
        )
        .unwrap();
        assert_eq!(pass["pass"], true, "{pass}");
    }

    #[test]
    fn review_hardcheck_supports_any_all_and_min() {
        let rubric = json!([
            {"id":"any","criterion":"任一项","check":"任一项","match":"any","must_include":["文件管理","进程管理"]},
            {"id":"all","criterion":"全部","check":"全部","match":"all","must_include":["文件管理","进程管理"]},
            {"id":"min","criterion":"至少两项","check":"至少两项","match":"min","min_matches":2,"must_include":["文件管理","进程管理","设备驱动"]}
        ]);
        let one = review_gate_plugin(
            "review.hardcheck",
            &json!({"answer":"只提到进程管理。", "rubric":rubric.clone()}),
        )
        .unwrap();
        assert_eq!(one["pass"], false, "{one}");
        let failed: Vec<&str> = one["failed"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|x| x.get("id").and_then(|v| v.as_str()))
            .collect();
        assert!(failed.contains(&"all"), "{one}");
        assert!(failed.contains(&"min"), "{one}");
        assert!(!failed.contains(&"any"), "{one}");
        let two = review_gate_plugin(
            "review.hardcheck",
            &json!({"answer":"文件管理、进程管理都有。", "rubric":rubric}),
        )
        .unwrap();
        assert_eq!(two["pass"], true, "{two}");
    }

    #[test]
    fn review_rubric_rejects_subjective_must_not_include() {
        let err = parse_rubric_with_input(
            r#"{"rubric":[
                {"id":"c1","criterion":"使用准确的术语","check":"确保术语和概念准确无误","must_not_include":["软件","硬件","计算机"]},
                {"id":"c2","criterion":"说明操作系统","check":"出现关键词","must_include":"操作系统"}
            ]}"#,
            "用三句话说明什么是操作系统",
        )
        .unwrap_err();
        assert!(err.contains("主观判据"), "{err}");
        assert!(err.contains("must_not_include"), "{err}");
    }

    #[test]
    fn review_rubric_rejects_banned_word_from_input() {
        let err = parse_rubric_with_input(
            r#"{"rubric":[
                {"id":"c1","criterion":"说明操作系统","check":"出现关键词","must_include":"操作系统"},
                {"id":"c2","criterion":"避免重复输入","check":"不要复述任务词","must_not_include":["操作系统"]}
            ]}"#,
            "用三句话说明什么是操作系统",
        )
        .unwrap_err();
        assert!(err.contains("出现在用户任务输入中"), "{err}");
    }

    #[test]
    fn review_rubric_moves_file_task_terms_to_judge() {
        let v = parse_rubric_with_input(
            r#"{"rubric":[
                {"id":"c1","criterion":"内存管理要点写入 workspace/notes2.txt","check":"核对 notes2.txt 与内存管理内容","must_include":["内存分配","内存回收","页面置换"],"match":"min"},
                {"id":"c2","criterion":"使用 tool.fs.write 写入文件","check":"核对写入工具成功记录"}
            ]}"#,
            "请用工具把三条关于内存管理的要点写入 workspace/notes2.txt，写完后不需要再读回",
        )
        .unwrap();
        assert_eq!(v["hard_items"], 0, "{v}");
        assert_eq!(v["file_task"], true, "{v}");
        assert!(
            v["judge_only"]
                .as_array()
                .unwrap()
                .iter()
                .any(|x| x == "c1"),
            "{v}"
        );
        assert!(v["rubric"][0].get("must_include").is_none(), "{v}");
        assert!(v["rubric"][1].get("judge_only").is_some(), "{v}");
    }

    #[test]
    fn review_rubric_file_task_bypasses_symbol_term_limit() {
        let v = parse_rubric_with_input(
            r#"{"rubric":[
                {"id":"c1","criterion":"notes3.txt 存在且包含三条内存管理要点，每条以 - 开头","check":"核对文件内容、行数与行首格式","must_include":["-"],"min_points":3,"pattern":"(?m)^- "},
                {"id":"c2","criterion":"使用 tool.fs.write 写入文件","check":"核对写入成功记录与文件正文"}
            ]}"#,
            "请用工具把三条关于「内存管理」的要点写入 workspace/notes3.txt，每条一行，以「- 」开头",
        )
        .unwrap();
        assert_eq!(v["file_task"], true, "{v}");
        assert_eq!(v["hard_items"], 0, "{v}");
        assert_eq!(v["rubric"][0]["judge_only"], true, "{v}");
        assert!(v["rubric"][0].get("must_include").is_none(), "{v}");
        assert!(v["rubric"][0].get("pattern").is_none(), "{v}");
        assert!(v["judge_only"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x == "c1"));
    }

    #[test]
    fn review_rubric_keeps_hard_items_for_code_tasks() {
        let v = parse_rubric_with_input(
            r#"{"rubric":[{"id":"c1","criterion":"实现 LRU 缓存","check":"答案包含 LRU","must_include":"LRU"}]}"#,
            "请用 Python 实现一个 LRU 缓存类，并给出代码",
        )
        .unwrap();
        assert_eq!(v["hard_items"], 1, "{v}");
        assert!(v["file_task"] == false, "{v}");
        assert!(v["rubric"][0].get("must_include").is_some(), "{v}");
    }
}
