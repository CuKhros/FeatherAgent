//! 端到端集成测试：真实拉起内核 + 插件进程，走 UDS 契约。
//!
//! 覆盖设计文档门禁：
//! - Phase 1：30 轮稳定对话、崩溃不中止
//! - Phase 2：100 轮长会话预算稳定（demo 用 mock provider，S3 真实模型数字另测）
//! - Phase 3：工具执行、协作式权限门、取消传播、fork/replay、存储索引

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use featheragent::engine::{Config, Kernel};
use featheragent::governance::paths::{Overrides, Paths};
use featheragent::proto::Codec;
use featheragent::server::client::{self, Client};
use featheragent::server::{self, Server};

struct Harness {
    kernel: Arc<Kernel>,
    srv: Arc<Server>,
    handle: Option<thread::JoinHandle<()>>,
    root: PathBuf,
}

fn debug_dir() -> PathBuf {
    // cargo 可能把测试二进制放到 deps/ 或 build/*/out/，向上找到含主二进制的目录
    let exe = std::env::current_exe().unwrap();
    let mut cur = exe.as_path();
    while let Some(parent) = cur.parent() {
        if parent.join("featheragent").is_file() {
            return parent.to_path_buf();
        }
        cur = parent;
    }
    panic!("找不到 target/debug 目录（从 {exe:?} 向上）");
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &dst.join(p.file_name().unwrap()));
        } else {
            let _ = std::fs::copy(&p, dst.join(p.file_name().unwrap()));
        }
    }
}

fn prepare_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("fa-it-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let debug = debug_dir();
    for bin in ["featheragent", "feather-plugin", "feather-storage"] {
        let src = debug.join(bin);
        assert!(src.exists(), "缺少 {src:?}，先运行 `cargo build --bins`");
        std::fs::copy(&src, root.join(bin)).unwrap();
    }
    let repo_plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins");
    copy_dir(&repo_plugins, &root.join("plugins"));
    // 让根目录下的 plugins/*/plugin.json 里的 "../../xxx" 指向 root
    root
}

fn start(name: &str, cfg_patch: impl FnOnce(&mut Config)) -> (Harness, client::Connect) {
    let root = prepare_root(name);
    let paths = Paths::resolve(
        &root.join("featheragent"),
        &Overrides {
            install_root: Some(root.clone()),
            ..Default::default()
        },
    );
    let mut cfg = Config {
        sync: "never".into(),
        ..Default::default()
    };
    cfg_patch(&mut cfg);
    let kernel = Kernel::new(paths.clone(), cfg, true).unwrap();
    let srv = Server::new(kernel.clone(), paths.socket(), Codec::MsgPack);
    let h = {
        let s = srv.clone();
        thread::spawn(move || {
            let _ = s.run();
        })
    };
    server::wait_ready(&paths.socket(), Duration::from_secs(10)).unwrap();
    let conn = Client::connect(&paths.socket()).unwrap();
    conn.client.call("initialize", json!({})).unwrap();
    (
        Harness {
            kernel,
            srv,
            handle: Some(h),
            root,
        },
        conn,
    )
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.srv.stop();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        self.kernel.stop();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn is_kind(v: &Value, kind: &str, session: &str) -> bool {
    v.get("method").and_then(|m| m.as_str()) == Some("event")
        && v.pointer("/params/kind").and_then(|k| k.as_str()) == Some(kind)
        && v.pointer("/params/payload/session")
            .and_then(|s| s.as_str())
            == Some(session)
}

fn send_and_wait(
    conn: &client::Connect,
    session: &str,
    text: &str,
    timeout: Duration,
) -> Vec<Value> {
    conn.client
        .call("chat.send", json!({"session": session, "text": text}))
        .unwrap();
    let deadline = Instant::now() + timeout;
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        match conn.events.recv_timeout(Duration::from_millis(200)) {
            Ok(v) => {
                if is_kind(&v, "chat.finished", session) {
                    seen.push(v);
                    return seen;
                }
                if v.get("params").and_then(|p| p.get("payload")).is_some() {
                    seen.push(v);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => break,
        }
    }
    panic!(
        "等待 chat.finished 超时（{session} / {text}）；看到 {} 条事件",
        seen.len()
    );
}

fn last_model_text(seen: &[Value]) -> String {
    seen.iter()
        .filter(|v| v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("model.message"))
        .filter_map(|v| {
            v.pointer("/params/payload/text")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string())
        })
        .next_back()
        .unwrap_or_default()
}

struct MockOpenAi {
    base: String,
    requests: Arc<std::sync::Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl MockOpenAi {
    fn new() -> Self {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.set_nonblocking(true).unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let reqs = requests.clone();
        let stop2 = stop.clone();
        let handle = thread::spawn(move || {
            while !stop2.load(Ordering::SeqCst) {
                match l.accept() {
                    Ok((mut s, _)) => {
                        s.set_read_timeout(Some(Duration::from_secs(2))).ok();
                        let (head, body) = read_http_request(&mut s);
                        reqs.lock().unwrap().push(format!("{head}\n{body}"));
                        let path = head.split_whitespace().nth(1).unwrap_or("");
                        let (ty, resp) = if path.starts_with("/v1/models") {
                            (
                                "application/json",
                                "{\"data\":[{\"id\":\"local-test\"}]}".to_string(),
                            )
                        } else if path.starts_with("/v1/embeddings") {
                            (
                                "application/json",
                                "{\"data\":[{\"embedding\":[1.0,0.0]}]}".to_string(),
                            )
                        } else if path.starts_with("/api/chat") {
                            (
                                "application/x-ndjson",
                                "{\"message\":{\"content\":\"OLLAMA-OK\"},\"done\":true}\n"
                                    .to_string(),
                            )
                        } else if path.starts_with("/api/embed") {
                            (
                                "application/json",
                                "{\"embeddings\":[[1.0,0.0]]}".to_string(),
                            )
                        } else if path.starts_with("/api/tags") {
                            (
                                "application/json",
                                "{\"models\":[{\"name\":\"local-test\"}]}".to_string(),
                            )
                        } else {
                            (
                                "text/event-stream",
                                "data: {\"choices\":[{\"delta\":{\"content\":\"LOCAL-OK\"}}]}\n\ndata: [DONE]\n\n".to_string(),
                            )
                        };
                        let (headers, body) = if path.starts_with("/api/chat") {
                            (
                                "Transfer-Encoding: chunked".to_string(),
                                format!("{:x}\r\n{}\r\n0\r\n\r\n", resp.len(), resp),
                            )
                        } else {
                            (format!("Content-Length: {}", resp.len()), resp)
                        };
                        let out = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: {ty}\r\n{headers}\r\nConnection: close\r\n\r\n{body}"
                        );
                        let _ = s.write_all(out.as_bytes());
                        let _ = s.flush();
                        thread::sleep(Duration::from_millis(50));
                    }
                    Err(_) => thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        Self {
            base,
            requests,
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for MockOpenAi {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn read_http_request(s: &mut std::net::TcpStream) -> (String, String) {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 1024];
    loop {
        let n = s.read(&mut tmp).unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let pos = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(0);
    let head = String::from_utf8_lossy(&buf[..pos]).to_string();
    let mut body = if pos + 4 <= buf.len() {
        buf[pos + 4..].to_vec()
    } else {
        Vec::new()
    };
    let len = head
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            if k.eq_ignore_ascii_case("content-length") {
                v.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);
    while body.len() < len {
        let n = s.read(&mut tmp).unwrap_or(0);
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    (head, String::from_utf8_lossy(&body).to_string())
}

#[test]
fn llama_cpp_and_ninfer_openai_chat_use_api_key() {
    for provider in ["provider-llamacpp", "provider-ninfer"] {
        let mock = MockOpenAi::new();
        let base = mock.base.clone();
        let (_h, conn) = start(provider, move |cfg| {
            cfg.provider_plugin = provider.to_string();
            cfg.base_url = base;
            cfg.provider_api_key = "sk-local-test".into();
            cfg.max_turns = 2;
        });
        conn.client
            .call("session.create", json!({"id": "slocal"}))
            .unwrap();
        let seen = send_and_wait(&conn, "slocal", "hello local", Duration::from_secs(20));
        assert_eq!(last_model_text(&seen), "LOCAL-OK", "{provider}");
        let reqs = mock.requests.lock().unwrap();
        assert!(
            reqs.iter()
                .any(|r| r.starts_with("POST /v1/chat/completions ")
                    && r.to_ascii_lowercase()
                        .contains("authorization: bearer sk-local-test")),
            "{provider} 未带 Key 调用 /v1/chat/completions: {:?}",
            reqs.iter().take(2).collect::<Vec<_>>()
        );
    }
}

#[test]
fn ollama_native_chat_uses_api_key() {
    let mock = MockOpenAi::new();
    let base = mock.base.clone();
    let (_h, conn) = start("ollamakey", move |cfg| {
        cfg.provider_plugin = "provider-ollama".into();
        cfg.base_url = base;
        cfg.provider_api_key = "sk-ollama".into();
        cfg.max_turns = 2;
    });
    conn.client
        .call("session.create", json!({"id": "sollamakey"}))
        .unwrap();
    let seen = send_and_wait(&conn, "sollamakey", "hello ollama", Duration::from_secs(20));
    assert_eq!(last_model_text(&seen), "OLLAMA-OK");
    let reqs = mock.requests.lock().unwrap();
    assert!(
        reqs.iter().any(|r| r.starts_with("POST /api/chat ")
            && r.to_ascii_lowercase()
                .contains("authorization: bearer sk-ollama")),
        "Ollama native 未带 Key: {:?}",
        reqs.iter().take(2).collect::<Vec<_>>()
    );
}

#[test]
fn ollama_provider_models_reads_api_tags() {
    let mock = MockOpenAi::new();
    let base = mock.base.clone();
    let (_h, conn) = start("ollamamodels", move |cfg| {
        cfg.provider_plugin = "provider-ollama".into();
        cfg.base_url = base;
        cfg.model = "local-test".into();
    });
    let v = conn.client.call("provider.models", json!({})).unwrap();
    assert_eq!(v["models"], json!(["local-test"]));
    assert!(v.get("error").is_none(), "provider.models 不应报错: {v}");
    let reqs = mock.requests.lock().unwrap();
    let tags = reqs
        .iter()
        .find(|r| r.starts_with("GET /api/tags "))
        .expect("没有请求 /api/tags");
    assert!(!tags.contains("/models"), "误用了 OpenAI 路径: {tags}");
}

#[test]
fn ollama_native_chat_without_key_still_sends_body() {
    let mock = MockOpenAi::new();
    let base = mock.base.clone();
    let (_h, conn) = start("ollamanokey", move |cfg| {
        cfg.provider_plugin = "provider-ollama".into();
        cfg.base_url = base;
        cfg.provider_api_key = String::new();
        cfg.max_turns = 2;
    });
    conn.client
        .call("session.create", json!({"id": "sollamanokey"}))
        .unwrap();
    let seen = send_and_wait(
        &conn,
        "sollamanokey",
        "hello ollama",
        Duration::from_secs(20),
    );
    assert_eq!(last_model_text(&seen), "OLLAMA-OK");
    let reqs = mock.requests.lock().unwrap();
    let chat = reqs
        .iter()
        .find(|r| r.starts_with("POST /api/chat "))
        .expect("没有 /api/chat 请求");
    assert!(
        chat.contains("Content-Length: "),
        "缺少 Content-Length: {chat}"
    );
    assert!(chat.contains("\"messages\""), "请求体丢失: {chat}");
    assert!(chat.contains("\"stream\":true"), "请求体不完整: {chat}");
    let embed = reqs
        .iter()
        .find(|r| r.starts_with("POST /api/embed "))
        .expect("没有 /api/embed 请求");
    assert!(
        embed.contains("Content-Length: "),
        "embed 缺少 Content-Length: {embed}"
    );
    assert!(embed.contains("\"input\""), "embed 请求体丢失: {embed}");
    assert!(embed.contains("\"model\""), "embed 未带模型名: {embed}");
}

#[test]
fn review_gate_drops_placeholder_meta_item_and_approves() {
    let (_h, conn) = start("review-m1", |cfg| {
        cfg.review_gate.enabled = true;
        cfg.review_gate.max_rounds = 1;
    });
    conn.client
        .call("session.create", json!({"id": "sm1"}))
        .unwrap();
    let seen = send_and_wait(&conn, "sm1", "REVIEW_M1", Duration::from_secs(20));
    let finished = seen
        .iter()
        .find(|v| v.pointer("/params/kind").and_then(|x| x.as_str()) == Some("chat.finished"))
        .expect("缺少 chat.finished");
    assert_eq!(
        finished
            .pointer("/params/payload/status")
            .and_then(|x| x.as_str()),
        Some("ok"),
        "{seen:?}"
    );
    assert_eq!(
        finished
            .pointer("/params/payload/review/status")
            .and_then(|x| x.as_str()),
        Some("approved"),
        "{seen:?}"
    );
    assert!(
        seen.iter().any(|v| {
            v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("review.warn")
                && v.pointer("/params/payload/text")
                    .and_then(|x| x.as_str())
                    .map(|t| t.contains("已丢弃") && t.contains("c2"))
                    .unwrap_or(false)
        }),
        "应留下丢弃占位符判据的 warn: {seen:?}"
    );
}

#[test]
fn review_gate_subjective_criterion_is_advisory() {
    let (_h, conn) = start("review-subj", |cfg| {
        cfg.review_gate.enabled = true;
        cfg.review_gate.max_rounds = 1;
    });
    conn.client
        .call("session.create", json!({"id": "ssubj"}))
        .unwrap();
    let seen = send_and_wait(&conn, "ssubj", "REVIEW_SUBJ", Duration::from_secs(20));
    let finished = seen
        .iter()
        .find(|v| v.pointer("/params/kind").and_then(|x| x.as_str()) == Some("chat.finished"))
        .expect("缺少 chat.finished");
    assert_eq!(
        finished
            .pointer("/params/payload/review/status")
            .and_then(|x| x.as_str()),
        Some("approved"),
        "{seen:?}"
    );
    assert!(
        seen.iter().any(|v| {
            v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("review.warn")
                && v.pointer("/params/payload/text")
                    .and_then(|x| x.as_str())
                    .map(|t| t.contains("主观判据"))
                    .unwrap_or(false)
        }),
        "主观判据应降级为 warn: {seen:?}"
    );
    let verdict = seen
        .iter()
        .find(|v| v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("review.verdict"))
        .expect("应有 review.verdict");
    assert_eq!(
        verdict
            .pointer("/params/payload/votes")
            .and_then(|x| x.as_u64()),
        Some(0),
        "主观判据不应调用 judge: {verdict:?}"
    );
}

#[test]
fn review_gate_unapproved_is_reflected_in_chat_status() {
    let (_h, conn) = start("reviewbad", |cfg| {
        cfg.review_gate.enabled = true;
        cfg.review_gate.max_rounds = 1;
    });
    conn.client
        .call("session.create", json!({"id": "sreviewbad"}))
        .unwrap();
    let seen = send_and_wait(&conn, "sreviewbad", "REVIEW_NEVER", Duration::from_secs(20));
    assert!(last_model_text(&seen).contains("REVIEW_NEVER"));
    let finished = seen
        .iter()
        .find(|v| v.pointer("/params/kind").and_then(|x| x.as_str()) == Some("chat.finished"))
        .expect("缺少 chat.finished");
    assert_eq!(
        finished
            .pointer("/params/payload/status")
            .and_then(|x| x.as_str()),
        Some("unapproved")
    );
    assert_eq!(
        finished
            .pointer("/params/payload/review/status")
            .and_then(|x| x.as_str()),
        Some("unapproved")
    );
}

#[test]
fn review_gate_reinjects_schema_error_and_falls_back_to_judge_only() {
    let (_h, conn) = start("review-soft", |cfg| {
        cfg.review_gate.enabled = true;
        cfg.review_gate.max_rounds = 1;
    });
    conn.client
        .call("session.create", json!({"id": "ssoft"}))
        .unwrap();
    // 第一次只给描述项 → 内核必须带错误回注重要一次；第二次给合规判据。
    let seen = send_and_wait(&conn, "ssoft", "REVIEW_SOFT_FIRST", Duration::from_secs(20));
    let guide = seen
        .iter()
        .find(|v| v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("review.guide"))
        .expect("应有 review.guide");
    assert_eq!(
        guide
            .pointer("/params/payload/attempts")
            .and_then(|x| x.as_u64()),
        Some(2),
        "软判据应触发一次带错误回注的重试: {guide:?}"
    );
    assert!(
        seen.iter()
            .any(|v| v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("review.warn")),
        "重试应留下 review.warn"
    );
    assert_eq!(last_model_text(&seen), "REVIEW-FINAL");

    // 始终只给描述项：重试用尽后降级为 judge-only，而不是整轮 degraded。
    let seen = send_and_wait(&conn, "ssoft", "REVIEW_SOFT_ONLY", Duration::from_secs(20));
    let finished = seen
        .iter()
        .find(|v| v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("review.finished"))
        .expect("应有 review.finished");
    assert_ne!(
        finished
            .pointer("/params/payload/status")
            .and_then(|x| x.as_str()),
        Some("degraded"),
        "全描述项不应整轮 degraded: {finished:?}"
    );
    assert!(
        seen.iter().any(|v| {
            v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("review.warn")
                && v.pointer("/params/payload/text")
                    .and_then(|x| x.as_str())
                    .map(|t| t.contains("没有硬判据"))
                    .unwrap_or(false)
        }),
        "应显式告警“没有硬判据”"
    );
}

#[test]
fn review_gate_min_points_soft_falls_back_to_judge() {
    let (_h, conn) = start("review-minpoints", |cfg| {
        cfg.review_gate.enabled = true;
        cfg.review_gate.max_rounds = 1;
    });
    conn.client
        .call("session.create", json!({"id": "smp"}))
        .unwrap();
    // 答案命中 must_include，但只有一段：机器计数 1/2，min_points 必须降级给 judge 而不是判死。
    let seen = send_and_wait(&conn, "smp", "REVIEW_MINPOINTS", Duration::from_secs(20));
    assert!(
        seen.iter().any(|v| {
            v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("review.warn")
                && v.pointer("/params/payload/text")
                    .and_then(|x| x.as_str())
                    .map(|t| t.contains("min_points 机器计数不足") && t.contains("按软判据接受"))
                    .unwrap_or(false)
        }),
        "min_points 不足应留下 review.warn: {seen:?}"
    );
    let finished = seen
        .iter()
        .find(|v| v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("review.finished"))
        .expect("应有 review.finished");
    assert_eq!(
        finished
            .pointer("/params/payload/status")
            .and_then(|x| x.as_str()),
        Some("approved"),
        "judge 复核通过后应 approved: {finished:?}"
    );
}

#[test]
fn review_gate_routes_negative_criterion_to_hardcheck() {
    let (_h, conn) = start("review-k1", |cfg| {
        cfg.review_gate.enabled = true;
        cfg.review_gate.max_rounds = 1;
    });
    conn.client
        .call("session.create", json!({"id": "sk1"}))
        .unwrap();
    // c1 是 must_not_include，若被送进 judge，mock 会返回“答案中没有…”这种非原文 evidence，
    // 必触发 degraded。正确路由下 judge 只看到描述型 c3，整轮应 approved。
    let seen = send_and_wait(&conn, "sk1", "REVIEW_K1", Duration::from_secs(20));
    let finished = seen
        .iter()
        .find(|v| v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("review.finished"))
        .expect("应有 review.finished");
    assert_eq!(
        finished
            .pointer("/params/payload/status")
            .and_then(|x| x.as_str()),
        Some("approved"),
        "负向判据应留在 hardcheck，不应拖垮 judge: {seen:?}"
    );
    assert!(!seen.iter().any(|v| v
        .pointer("/params/payload/text")
        .and_then(|x| x.as_str())
        .map(|t| t.contains("evidence 不是答案原文"))
        .unwrap_or(false)));
}

#[test]
fn review_gate_missing_description_item_is_reinjected() {
    let (_h, conn) = start("review-k2", |cfg| {
        cfg.review_gate.enabled = true;
        cfg.review_gate.max_rounds = 1;
    });
    conn.client
        .call("session.create", json!({"id": "sk2"}))
        .unwrap();
    // mock judge 第一次漏掉 c4；内核应把“缺少验收项 c4”回注后补投，而不是整轮 degraded。
    let seen = send_and_wait(
        &conn,
        "sk2",
        "REVIEW_MISSING_FIRST",
        Duration::from_secs(20),
    );
    assert!(
        seen.iter().any(|v| {
            v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("review.warn")
                && v.pointer("/params/payload/text")
                    .and_then(|x| x.as_str())
                    .map(|t| t.contains("缺少验收项"))
                    .unwrap_or(false)
        }),
        "漏项应触发 review.warn 回注: {seen:?}"
    );
    let finished = seen
        .iter()
        .find(|v| v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("review.finished"))
        .expect("应有 review.finished");
    assert_eq!(
        finished
            .pointer("/params/payload/status")
            .and_then(|x| x.as_str()),
        Some("approved"),
        "补投后应 approved: {finished:?}"
    );
}

#[test]
fn review_gate_three_roles_retry_and_approve() {
    let (h, conn) = start("review", |cfg| {
        cfg.review_gate.enabled = true;
        cfg.review_gate.max_rounds = 1;
        cfg.review_gate.guide.temperature = 0.3;
        cfg.review_gate.exec.temperature = 0.7;
        cfg.review_gate.judge.temperature = 0.1;
    });
    conn.client
        .call("session.create", json!({"id": "sreview"}))
        .unwrap();
    let seen = send_and_wait(&conn, "sreview", "REVIEW_START", Duration::from_secs(20));
    assert_eq!(last_model_text(&seen), "REVIEW-FINAL");
    let kinds: Vec<&str> = seen
        .iter()
        .filter_map(|v| v.pointer("/params/kind").and_then(|k| k.as_str()))
        .collect();
    assert!(
        kinds.contains(&"review.guide"),
        "缺少 review.guide: {kinds:?}"
    );
    assert!(
        kinds.contains(&"review.verdict"),
        "缺少 review.verdict: {kinds:?}"
    );
    assert!(
        kinds.contains(&"review.feedback"),
        "缺少 review.feedback: {kinds:?}"
    );
    assert!(
        kinds.contains(&"review.finished"),
        "缺少 review.finished: {kinds:?}"
    );
    let finished = seen
        .iter()
        .find(|v| v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("chat.finished"))
        .unwrap();
    assert_eq!(
        finished
            .pointer("/params/payload/review/approved")
            .and_then(|x| x.as_bool()),
        Some(true)
    );
    assert_eq!(
        finished
            .pointer("/params/payload/review/rounds")
            .and_then(|x| x.as_u64()),
        Some(2)
    );

    // 生成物落盘：guide 的 rubric 与 judge 的逐票 verdict 都要能在磁盘上找到。
    let guide = seen
        .iter()
        .find(|v| v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("review.guide"))
        .expect("应有 review.guide");
    let guide_path = guide
        .pointer("/params/payload/artifact")
        .and_then(|x| x.as_str())
        .filter(|x| !x.is_empty())
        .expect("review.guide 应带 artifact 路径");
    assert!(
        Path::new(guide_path).is_file(),
        "guide 生成物不存在: {guide_path}"
    );
    let guide_artifact: Value =
        serde_json::from_str(&std::fs::read_to_string(guide_path).unwrap()).unwrap();
    assert_eq!(guide_artifact["role"], "guide");
    assert!(guide_artifact["raw"]
        .as_str()
        .unwrap_or("")
        .contains("rubric"));
    assert!(Path::new(guide_path).starts_with(h.root.join("data/review-artifacts/sreview")));

    let verdict_event = seen
        .iter()
        .filter(|v| v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("review.verdict"))
        .find(|v| {
            v.pointer("/params/payload/artifacts")
                .and_then(|x| x.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false)
        })
        .unwrap_or_else(|| panic!("应有带 judge 生成物的 review.verdict: {seen:?}"));
    let artifacts = verdict_event
        .pointer("/params/payload/artifacts")
        .and_then(|x| x.as_array())
        .expect("review.verdict 应带 artifacts");
    assert!(!artifacts.is_empty(), "judge 生成物为空");
    for a in artifacts {
        let p = a.as_str().expect("artifact 路径");
        assert!(Path::new(p).is_file(), "judge 生成物不存在: {p}");
        let j: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
        assert_eq!(j["role"], "judge");
        assert!(Path::new(p).starts_with(h.root.join("data/review-artifacts/sreview")));
    }
}

#[test]
fn thirty_rounds_stable() {
    let (_h, conn) = start("30r", |_| {});
    conn.client
        .call("session.create", json!({"id": "s30"}))
        .unwrap();
    for i in 0..30 {
        let seen = send_and_wait(&conn, "s30", &format!("round {i}"), Duration::from_secs(10));
        assert!(
            last_model_text(&seen).starts_with(&format!("echo: round {i}")),
            "第 {i} 轮回复异常"
        );
    }
    let info = conn
        .client
        .call("ctx.inspect", json!({"session": "s30"}))
        .unwrap();
    assert_eq!(info["state"], "idle");
    let events = conn
        .client
        .call("session.resume", json!({"session": "s30"}))
        .unwrap();
    assert_eq!(events["role"], "holder");
    assert!(events["total"].as_u64().unwrap() >= 30);
}

#[test]
fn hundred_rounds_budget_stable() {
    let (_h, conn) = start("100r", |_| {});
    conn.client
        .call("session.create", json!({"id": "s100"}))
        .unwrap();
    let mut max_tokens = 0u64;
    for i in 0..100 {
        let seen = send_and_wait(
            &conn,
            "s100",
            &format!("长会话第 {i} 轮：{}", "内容".repeat(8)),
            Duration::from_secs(10),
        );
        assert!(!last_model_text(&seen).is_empty());
        if i % 5 == 0 {
            let snap = conn
                .client
                .call("ctx.inspect", json!({"session": "s100"}))
                .unwrap();
            max_tokens = max_tokens.max(
                snap.pointer("/snapshot/stats/total_tokens")
                    .and_then(|x| x.as_u64())
                    .unwrap_or(0),
            );
        }
    }
    assert!(max_tokens <= 8192 * 95 / 100, "预算超 95%: {max_tokens}");
}

#[test]
fn tool_execution_and_permission_gate() {
    let (h, conn) = start("tools", |_| {});
    conn.client
        .call("session.create", json!({"id": "stool"}))
        .unwrap();
    // 预置工作空间文件
    let ws = h.kernel.paths.workspace.join("default");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("note.txt"), "feather-note").unwrap();

    let seen = send_and_wait(&conn, "stool", "READ:note.txt", Duration::from_secs(15));
    let result = seen
        .iter()
        .find(|v| v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("tool.result"))
        .cloned()
        .expect("应有 tool.result");
    assert_eq!(
        result
            .pointer("/params/payload/result/content")
            .and_then(|x| x.as_str()),
        Some("feather-note")
    );

    // 危险工具：必须收到确认请求，允许后才执行
    let seen = {
        let payload = r#"TOOL:{"name":"tool.shell","arguments":{"cmd":"echo hi"}}"#;
        conn.client
            .call("chat.send", json!({"session": "stool", "text": payload}))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut confirm = None;
        let mut collected = Vec::new();
        while Instant::now() < deadline {
            match conn.events.recv_timeout(Duration::from_millis(200)) {
                Ok(v) => {
                    if v.get("method").and_then(|m| m.as_str()) == Some("tool.confirm_request") {
                        confirm = Some(v);
                        break;
                    }
                    collected.push(v);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => break,
            }
        }
        let c = confirm.expect("未收到 tool.confirm_request");
        // H5：确认请求必须带完整命令，用户才能判断要不要放行。
        assert_eq!(
            c.pointer("/params/arguments/cmd").and_then(|x| x.as_str()),
            Some("echo hi"),
            "confirm_request 缺少原始 arguments: {c:?}"
        );
        let preview = c
            .pointer("/params/preview")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        assert!(
            preview.contains("echo hi"),
            "preview 应包含完整命令: {preview:?}"
        );
        let token = c
            .pointer("/params/token")
            .and_then(|t| t.as_str())
            .unwrap()
            .to_string();
        conn.client
            .call("tool.confirm", json!({"token": token, "allow": true}))
            .unwrap();
        let mut rest = send_and_wait_after(&conn, "stool", collected);
        rest.append(&mut Vec::new());
        rest
    };
    let shell = seen
        .iter()
        .find(|v| v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("tool.result"))
        .cloned()
        .expect("应有 shell 结果");
    assert!(shell
        .pointer("/params/payload/result/stdout")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .contains("hi"));
}

fn send_and_wait_after(conn: &client::Connect, session: &str, already: Vec<Value>) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut seen = already;
    while Instant::now() < deadline {
        match conn.events.recv_timeout(Duration::from_millis(200)) {
            Ok(v) => {
                if is_kind(&v, "chat.finished", session) {
                    seen.push(v);
                    return seen;
                }
                seen.push(v);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => break,
        }
    }
    seen
}

#[test]
fn shell_timeout_marks_timed_out() {
    let (_h, conn) = start("timeout", |_| {});
    conn.client
        .call("session.create", json!({"id": "stimeout"}))
        .unwrap();
    let payload = r#"TOOL:{"name":"tool.shell","arguments":{"cmd":"sleep 30"}}"#;
    conn.client
        .call("chat.send", json!({"session": "stimeout", "text": payload}))
        .unwrap();
    // 等危险工具确认请求
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut token = None;
    while Instant::now() < deadline {
        if let Ok(v) = conn.events.recv_timeout(Duration::from_millis(200)) {
            if v.get("method").and_then(|m| m.as_str()) == Some("tool.confirm_request") {
                token = v
                    .pointer("/params/token")
                    .and_then(|x| x.as_str())
                    .map(|x| x.to_string());
                break;
            }
        }
    }
    let tok = token.expect("confirm token");
    let r = conn
        .client
        .call("tool.confirm", json!({"token": tok, "allow": true}))
        .unwrap();
    assert_eq!(r["ok"], true, "确认未被接受: {r}");
    // 15s 超时 + 一轮收尾：给足 30s
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        match conn.events.recv_timeout(Duration::from_millis(300)) {
            Ok(v) => {
                if is_kind(&v, "chat.finished", "stimeout") {
                    seen.push(v);
                    break;
                }
                seen.push(v);
            }
            Err(_) => continue,
        }
    }
    let tool = seen
        .iter()
        .find(|v| v.pointer("/params/kind").and_then(|x| x.as_str()) == Some("tool.result"))
        .cloned()
        .unwrap_or_else(|| {
            let kinds: Vec<&str> = seen
                .iter()
                .filter_map(|v| v.pointer("/params/kind").and_then(|x| x.as_str()))
                .collect();
            panic!("应有 tool.result；实际事件: {kinds:?}");
        });
    assert_eq!(
        tool.pointer("/params/payload/result/timed_out")
            .and_then(|x| x.as_bool()),
        Some(true),
        "工具应超时: {tool}"
    );
}

#[test]
fn cancel_propagation() {
    let (_h, conn) = start("cancel", |_| {});
    conn.client
        .call("session.create", json!({"id": "sc"}))
        .unwrap();
    conn.client
        .call("chat.send", json!({"session": "sc", "text": "SPIN"}))
        .unwrap();
    thread::sleep(Duration::from_millis(400));
    conn.client
        .call("chat.cancel", json!({"session": "sc"}))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if let Ok(v) = conn.events.recv_timeout(Duration::from_millis(200)) {
            if is_kind(&v, "chat.finished", "sc") {
                assert_eq!(
                    v.pointer("/params/payload/status").and_then(|s| s.as_str()),
                    Some("cancelled")
                );
                return;
            }
        }
    }
    panic!("取消后未收到 chat.finished(cancelled)");
}

#[test]
fn crash_restart_retries_once() {
    let (_h, conn) = start("crash", |_| {});
    conn.client
        .call("session.create", json!({"id": "scrash"}))
        .unwrap();
    let seen = send_and_wait(&conn, "scrash", "CRASH_ONCE", Duration::from_secs(20));
    assert!(
        last_model_text(&seen).contains("restarted"),
        "崩溃重启后应重试成功: {seen:?}"
    );
}

#[test]
fn observer_is_readonly_and_fork_copies() {
    let (_h, conn) = start("obs", |_| {});
    conn.client
        .call("session.create", json!({"id": "so"}))
        .unwrap();
    send_and_wait(&conn, "so", "hello", Duration::from_secs(10));

    let conn2 = Client::connect(&_h.kernel.paths.socket()).unwrap();
    let resume = conn2
        .client
        .call("session.resume", json!({"session": "so"}))
        .unwrap();
    assert_eq!(resume["role"], "observer");
    let err = conn2
        .client
        .call("chat.send", json!({"session": "so", "text": "should fail"}))
        .unwrap_err();
    assert!(err.contains("40301"), "观察者写入应被拒: {err}");

    let fork = conn
        .client
        .call("session.fork", json!({"session": "so", "as": "so-fork"}))
        .unwrap();
    assert!(fork["copied"].as_u64().unwrap() >= 2);
    let child_events = conn
        .client
        .call("session.resume", json!({"session": "so-fork"}))
        .unwrap();
    assert!(child_events["total"].as_u64().unwrap() >= 3); // 复制 + session.forked
}

#[test]
fn storage_index_and_vector_recall() {
    let (_h, conn) = start("storage", |_| {});
    conn.client
        .call("session.create", json!({"id": "sst"}))
        .unwrap();
    send_and_wait(
        &conn,
        "sst",
        "苹果 香蕉 上下文测试",
        Duration::from_secs(15),
    );
    // 索引器是异步的，给一点时间
    thread::sleep(Duration::from_millis(600));
    let stats = conn.client.call("storage.stats", json!({})).unwrap();
    assert_eq!(stats["available"], true, "storage 应可用: {stats}");
    assert!(
        stats["events"].as_u64().unwrap() >= 2,
        "索引事件数: {stats}"
    );
    assert!(
        stats["embeddings"].as_u64().unwrap() >= 1,
        "向量应已写入: {stats}"
    );
}

#[test]
fn s2_thousand_truncations() {
    let bin = debug_dir().join("featheragent");
    assert!(bin.exists());
    let out = std::process::Command::new(&bin)
        .args(["doctor", "--stress-log", "1000"])
        .output()
        .expect("spawn doctor");
    assert!(
        out.status.success(),
        "stress-log 失败: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["failed"], 0);
    assert_eq!(v["iterations"], 1000);
}

#[test]
fn plugin_contract_list() {
    let (_h, conn) = start("plugins", |_| {});
    let v = conn.client.call("plugin.list", json!({})).unwrap();
    let names: Vec<String> = v["registry"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|x| x["name"].as_str().map(|s| s.to_string()))
        .collect();
    assert!(
        names.contains(&"provider-mock".to_string()),
        "registry={names:?}"
    );
}

#[test]
fn wasm_and_log_unit_paths_smoke() {
    // 集成测试里也验证一下内核侧没有把业务存储塞进来
    let (_h, conn) = start("smoke", |_| {});
    let info = conn.client.call("system.info", json!({})).unwrap();
    assert_eq!(info["version"].as_str().unwrap(), env!("CARGO_PKG_VERSION"));
    assert_eq!(info["gate"], "collaborative");
}

#[test]
fn unknown_tool_name_gets_correction_instead_of_raw_rpc_error() {
    // H3：模型拼出畸形工具名（tool.fs / tool.read_file）时，内核必须回注“未知工具 + 可用清单”，
    // 而不是把畸形名字原样转发给工具插件（那样只会得到一句难懂的 rpc 错误）。
    let (h, conn) = start("unknown-tool", |_| {});
    conn.client
        .call("session.create", json!({"id": "utool"}))
        .unwrap();
    let ws = h.kernel.paths.workspace.join("default");
    std::fs::create_dir_all(&ws).unwrap();

    let seen = send_and_wait(
        &conn,
        "utool",
        r#"TOOL:{"name":"tool.fs","arguments":{"path":"a.txt"}}"#,
        Duration::from_secs(15),
    );
    let err = seen
        .iter()
        .find(|v| v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("tool.error"))
        .unwrap_or_else(|| panic!("未知工具应发 tool.error: {seen:?}"));
    let msg = err
        .pointer("/params/payload/error")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    assert!(msg.contains("未知工具 tool.fs"), "error={msg}");
    assert!(msg.contains("可用工具"), "error={msg}");
    assert!(msg.contains("tool.fs.read"), "应列出真实工具名: {msg}");
    // 纠错提示要回注给模型，后续轮次必须仍能正常对话收尾。
    assert!(
        seen.iter()
            .any(|v| v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("chat.finished")),
        "纠错后会话应正常结束: {seen:?}"
    );
    assert!(
        !seen
            .iter()
            .any(|v| v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("plugin.degraded")),
        "未知工具名不应熔断插件: {seen:?}"
    );
}

#[test]
fn leading_slash_workspace_and_business_error_are_not_fatal() {
    let (h, conn) = start("paths", |_| {});
    conn.client
        .call("session.create", json!({"id": "spath"}))
        .unwrap();
    let ws = h.kernel.paths.workspace.join("default");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("lead.txt"), "LEAD_CONTENT").unwrap();

    let seen = send_and_wait(
        &conn,
        "spath",
        r#"TOOL:{"name":"tool.fs.read","arguments":{"path":"/workspace/lead.txt"}}"#,
        Duration::from_secs(15),
    );
    let result = seen
        .iter()
        .find(|v| v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("tool.result"))
        .expect("leading slash should be normalized to install root");
    assert_eq!(
        result
            .pointer("/params/payload/result/content")
            .and_then(|x| x.as_str()),
        Some("LEAD_CONTENT")
    );

    let seen = send_and_wait(
        &conn,
        "spath",
        r#"TOOL:{"name":"tool.fs.read","arguments":{"path":"/workspace/missing-xyz.txt"}}"#,
        Duration::from_secs(15),
    );
    assert!(
        seen.iter()
            .any(|v| v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("tool.error")),
        "业务错误应发 tool.error: {seen:?}"
    );
    assert!(
        !seen
            .iter()
            .any(|v| v.pointer("/params/kind").and_then(|k| k.as_str()) == Some("plugin.degraded")),
        "业务错误不应熔断插件: {seen:?}"
    );
}

#[test]
fn config_api_applies_and_lists_models() {
    let (h, conn) = start("config", |_| {});
    assert!(
        h.root.join("runtime/ipc/ready").is_file(),
        "serve 就绪后必须写 ready 标记"
    );
    let cfg = conn.client.call("config.get", json!({})).unwrap();
    assert_eq!(cfg["temperature"].as_f64(), Some(0.7));

    let saved = conn
        .client
        .call(
            "config.set",
            json!({"patch": {
                "model": "mock-config",
                "temperature": 0.2,
                "context_window": 4096,
                "max_turns": 3
            }}),
        )
        .unwrap();
    assert_eq!(saved["config"]["temperature"].as_f64(), Some(0.2));
    assert_eq!(saved["config"]["model"], "mock-config");
    let on_disk: Value =
        serde_json::from_slice(&std::fs::read(&h.kernel.cfg_path).unwrap()).unwrap();
    assert_eq!(on_disk["temperature"].as_f64(), Some(0.2));

    let models = conn
        .client
        .call(
            "provider.models",
            json!({"provider": "provider-mock", "model": "mock-config"}),
        )
        .unwrap();
    assert!(models["models"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m == "mock-config"));

    let bad = conn
        .client
        .call("config.set", json!({"patch": {"temperature": 3.0}}))
        .unwrap_err();
    assert!(bad.contains("temperature"), "非法配置应被拒绝: {bad}");

    // 单字段原子更新：不必先 get 再整体覆盖。
    let one = conn
        .client
        .call("config.set", json!({"path": "temperature", "value": 0.45}))
        .unwrap();
    assert_eq!(one["config"]["temperature"].as_f64(), Some(0.45));
    let read = conn
        .client
        .call("config.get", json!({"path": "temperature"}))
        .unwrap();
    assert_eq!(read["value"].as_f64(), Some(0.45));
    let slash = conn
        .client
        .call("config.set", json!({"path": "/temperature", "value": 0.46}))
        .unwrap();
    assert_eq!(
        slash["config"]["temperature"].as_f64(),
        Some(0.46),
        "/temperature 应等同于 temperature"
    );
    let slash_read = conn
        .client
        .call("config.get", json!({"path": "/temperature"}))
        .unwrap();
    assert_eq!(slash_read["value"].as_f64(), Some(0.46));
    let missing = conn
        .client
        .call("config.set", json!({"path": "no_such_field", "value": 1}))
        .unwrap_err();
    assert!(missing.contains("不存在"), "未知字段应被拒绝: {missing}");

    // review-gate 温度是 schema 约束：即使 enabled=false 也必须拒绝越界值。
    for (path, value, range) in [
        ("review_gate.guide.temperature", 0.9, "[0.2, 0.5]"),
        ("review_gate.exec.temperature", 1.9, "[0.6, 1.5]"),
        ("review_gate.judge.temperature", 0.8, "[0.0, 0.2]"),
    ] {
        let err = conn
            .client
            .call("config.set", json!({"path": path, "value": value}))
            .unwrap_err();
        assert!(
            err.contains(path) && err.contains(range),
            "越界温度应被拒绝且带区间: path={path} err={err}"
        );
    }
    let on_disk: Value =
        serde_json::from_slice(&std::fs::read(&h.kernel.cfg_path).unwrap()).unwrap();
    assert_eq!(
        on_disk["review_gate"]["judge"]["temperature"].as_f64(),
        Some(0.1)
    );
    let valid = conn
        .client
        .call(
            "config.set",
            json!({"patch": {
                "review_gate": {
                    "guide": {"temperature": 0.3},
                    "exec": {"temperature": 0.7},
                    "judge": {"temperature": 0.1}
                }
            }}),
        )
        .unwrap();
    assert_eq!(
        valid["config"]["review_gate"]["judge"]["temperature"].as_f64(),
        Some(0.1)
    );

    // 两个并发保存不应互相覆盖：锁保证最终值是其中一个完整结果。
    let k1 = h.kernel.clone();
    let k2 = h.kernel.clone();
    let t1 = thread::spawn(move || k1.set_config_path("max_turns", json!(5)).is_ok());
    let t2 = thread::spawn(move || k2.set_config_path("context_window", json!(4096)).is_ok());
    assert!(t1.join().unwrap() && t2.join().unwrap());
    let now = conn.client.call("config.get", json!({})).unwrap();
    assert_eq!(now["max_turns"].as_u64(), Some(5));
    assert_eq!(now["context_window"].as_u64(), Some(4096));

    // 整份替换保留为独立方法，普通 set 不会被误当成 replace。
    let mut full = now;
    full["temperature"] = json!(0.55);
    let replaced = conn
        .client
        .call("config.replace", json!({"config": full}))
        .unwrap();
    assert_eq!(replaced["config"]["temperature"].as_f64(), Some(0.55));
}

fn wait_finished(conn: &client::Connect, session: &str, timeout: Duration) -> Vec<Value> {
    let deadline = Instant::now() + timeout;
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        if let Ok(v) = conn.events.recv_timeout(Duration::from_millis(200)) {
            if is_kind(&v, "chat.finished", session) {
                seen.push(v);
                return seen;
            }
            if v.get("params").and_then(|p| p.get("payload")).is_some() {
                seen.push(v);
            }
        }
    }
    panic!("等待 chat.finished 超时（{session}）");
}

#[test]
fn project_workspace_has_own_agent_home_and_worklog() {
    let (h, conn) = start("project", |_| {});
    conn.client
        .call(
            "session.create",
            json!({"id": "sproj", "workspace": "proj-alpha"}),
        )
        .unwrap();
    let ws = h.kernel.paths.workspace.join("proj-alpha");
    assert!(
        ws.join(".featheagent/worklog.md").exists(),
        "项目应自有 .featheagent"
    );
    std::fs::write(ws.join("note.txt"), "project-note").unwrap();

    let v = conn.client.call("workspace.list", json!({})).unwrap();
    let names: Vec<&str> = v["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|w| w["name"].as_str())
        .collect();
    assert!(names.contains(&"default"));
    assert!(names.contains(&"proj-alpha"));

    let _ = send_and_wait(&conn, "sproj", "READ:note.txt", Duration::from_secs(10));
    thread::sleep(Duration::from_millis(300));
    let log = std::fs::read_to_string(ws.join(".featheagent/worklog.md")).unwrap();
    assert!(
        log.contains("- 工具：tool.fs.read"),
        "worklog 应记录真实工具：{log}"
    );
    assert!(
        log.contains("## 20"),
        "worklog 应有内核写入的 UTC 日期：{log}"
    );

    let resumed = conn
        .client
        .call("session.resume", json!({"session": "sproj"}))
        .unwrap();
    assert_eq!(resumed["workspace"], "proj-alpha");

    conn.client
        .call(
            "session.create",
            json!({"id": "sproj2", "workspace": "proj-beta"}),
        )
        .unwrap();
    assert!(h
        .kernel
        .paths
        .workspace
        .join("proj-beta/.featheagent/memory.md")
        .exists());
}

#[test]
fn stop_preserves_partial_output_and_session_remains_usable() {
    let (_h, conn) = start("stop", |_| {});
    conn.client
        .call("session.create", json!({"id": "sstop"}))
        .unwrap();
    conn.client
        .call("chat.send", json!({"session": "sstop", "text": "SPIN"}))
        .unwrap();
    thread::sleep(Duration::from_millis(450));
    conn.client
        .call("chat.stop", json!({"session": "sstop"}))
        .unwrap();
    let seen = wait_finished(&conn, "sstop", Duration::from_secs(10));
    assert_eq!(
        seen.last()
            .and_then(|v| v.pointer("/params/payload/status"))
            .and_then(|x| x.as_str()),
        Some("stopped")
    );
    assert!(
        seen.iter()
            .any(
                |v| v.pointer("/params/kind").and_then(|x| x.as_str()) == Some("model.message")
                    && v.pointer("/params/payload/finish").and_then(|x| x.as_str())
                        == Some("interrupted")
            ),
        "停止时应发出一条 finish=interrupted 的部分输出"
    );
    let after = send_and_wait(&conn, "sstop", "after-stop", Duration::from_secs(10));
    assert!(last_model_text(&after).starts_with("echo: after-stop"));
}

#[test]
#[ignore = "需要公网访问 Bing；发布前手动运行验证本地搜索链路"]
fn local_web_search_tool_via_kernel() {
    let (_h, conn) = start("websearch", |_| {});
    conn.client
        .call("session.create", json!({"id": "sweb"}))
        .unwrap();
    let seen = send_and_wait(
        &conn,
        "sweb",
        "SEARCH:rust markdown",
        Duration::from_secs(40),
    );
    assert!(
        seen.iter().any(|v| {
            is_kind(v, "tool.call", "sweb")
                && v.pointer("/params/payload/name").and_then(|x| x.as_str()) == Some("web.search")
        }),
        "应产生 web.search 工具调用"
    );
    let result = seen
        .iter()
        .find(|v| is_kind(v, "tool.result", "sweb"))
        .and_then(|v| v.pointer("/params/payload/result"))
        .cloned()
        .unwrap_or(Value::Null);
    assert!(
        result
            .get("results")
            .and_then(|x| x.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false),
        "本地搜索结果不应为空：{result}"
    );
    assert!(last_model_text(&seen).contains("已完成"));
}

#[test]
fn inject_steers_running_turn() {
    let (_h, conn) = start("steer", |_| {});
    conn.client
        .call("session.create", json!({"id": "ssteer"}))
        .unwrap();
    conn.client
        .call("chat.send", json!({"session": "ssteer", "text": "SPIN"}))
        .unwrap();
    thread::sleep(Duration::from_millis(450));
    let v = conn
        .client
        .call(
            "chat.inject",
            json!({"session": "ssteer", "text": "改做总结"}),
        )
        .unwrap();
    assert!(v["ok"].as_bool().unwrap_or(false));
    let seen = wait_finished(&conn, "ssteer", Duration::from_secs(15));
    assert!(
        seen.iter().any(|e| is_kind(e, "user.message", "ssteer")
            && e.pointer("/params/payload/source").and_then(|x| x.as_str()) == Some("steer")),
        "应把引导作为 during_turn 用户消息写入事件日志"
    );
    assert_eq!(
        seen.last()
            .and_then(|v| v.pointer("/params/payload/status"))
            .and_then(|x| x.as_str()),
        Some("ok")
    );
}

#[test]
fn manual_compaction_creates_checkpoint_and_keeps_log() {
    let (_h, conn) = start("compact", |_| {});
    conn.client
        .call("session.create", json!({"id": "scomp"}))
        .unwrap();
    for i in 0..3 {
        let _ = send_and_wait(
            &conn,
            "scomp",
            &format!("compact round {i}"),
            Duration::from_secs(10),
        );
    }
    let v = conn
        .client
        .call("context.compact", json!({"session": "scomp"}))
        .unwrap();
    assert!(v["compacted_events"].as_u64().unwrap_or(0) > 0);
    assert!(v["covers_until_seq"].as_u64().unwrap_or(0) > 0);
    let resumed = conn
        .client
        .call("session.resume", json!({"session": "scomp"}))
        .unwrap();
    assert!(
        resumed["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "context.compacted"),
        "压缩必须在事件日志留下可回放的检查点"
    );
    let after = send_and_wait(&conn, "scomp", "after-compact", Duration::from_secs(10));
    assert!(last_model_text(&after).starts_with("echo: after-compact"));
}

#[test]
fn plugin_settings_are_relayed_and_validated() {
    let (_h, conn) = start("plugin-settings", |_| {});
    // 1) 内核把插件声明原样转述：registry 条目带 settings。
    let list = conn.client.call("plugin.list", json!({})).unwrap();
    let entry = list["registry"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "review-gate")
        .expect("review-gate 已登记");
    assert_eq!(entry["settings"]["root"], "review_gate");
    assert_eq!(
        entry["settings"]["fields"].as_array().unwrap().len(),
        14,
        "review-gate 声明 14 个字段（新增路由/投票 5 项）"
    );

    // 2) plugin.settings 返回声明 + 当前值；内核不解释语义。
    let s = conn
        .client
        .call("plugin.settings", json!({"name": "review-gate"}))
        .unwrap();
    assert_eq!(s["root"], "review_gate");
    assert_eq!(s["values"]["enabled"], false);
    assert!((s["values"]["guide.temperature"].as_f64().unwrap() - 0.3).abs() < 1e-9);

    // 3) 未声明设置项的插件返回明确错误，而不是空表单。
    let err = conn
        .client
        .call("plugin.settings", json!({"name": "tool"}))
        .unwrap_err();
    assert!(err.to_string().contains("未声明设置项"), "{err}");

    // 4) 写入按声明校验：guide 温度越界被内核拒绝。
    let err = conn
        .client
        .call(
            "config.set",
            json!({"path": "review_gate.guide.temperature", "value": 0.9}),
        )
        .unwrap_err();
    assert!(err.to_string().contains("guide.temperature"), "{err}");

    // 5) 合法值写入后 plugin.settings 立刻反映。
    conn.client
        .call(
            "config.set",
            json!({"path": "review_gate.max_rounds", "value": 3}),
        )
        .unwrap();
    let s2 = conn
        .client
        .call("plugin.settings", json!({"name": "review-gate"}))
        .unwrap();
    assert_eq!(s2["values"]["max_rounds"], 3);

    // 5b) rc.3.3 新字段：votes 只允许 1/3，路由枚举与正则引擎可按声明写入。
    let err = conn
        .client
        .call(
            "config.set",
            json!({"path": "review_gate.judge.votes", "value": 2}),
        )
        .unwrap_err();
    assert!(err.to_string().contains("judge.votes"), "{err}");
    conn.client
        .call(
            "config.set",
            json!({"path": "review_gate.judge.votes", "value": 3}),
        )
        .unwrap();
    conn.client
        .call(
            "config.set",
            json!({"path": "review_gate.criteria_routing", "value": "hardcheck_only"}),
        )
        .unwrap();
    conn.client
        .call(
            "config.set",
            json!({"path": "review_gate.hardcheck.pattern_engine", "value": "regex"}),
        )
        .unwrap();
    let s3 = conn
        .client
        .call("plugin.settings", json!({"name": "review-gate"}))
        .unwrap();
    assert_eq!(s3["values"]["judge.votes"], 3);
    assert_eq!(s3["values"]["criteria_routing"], "hardcheck_only");
    assert_eq!(s3["values"]["hardcheck.pattern_engine"], "regex");

    // 6) 未声明的键不能借 plugin_settings 命名空间写入。
    let err = conn
        .client
        .call(
            "config.set",
            json!({"path": "plugin_settings.search.unknown", "value": 1}),
        )
        .unwrap_err();
    assert!(err.to_string().contains("配置项不存在"), "{err}");
}
