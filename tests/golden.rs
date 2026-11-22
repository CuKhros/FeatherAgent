//! golden trace 回归（§9.1）：固定 fixture，逐字段断言解析与整条管线。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};

use featheragent::context::parse;
use featheragent::engine::{Config, Kernel};
use featheragent::governance::paths::{Overrides, Paths};
use featheragent::proto::Codec;
use featheragent::server::client::{self, Client};
use featheragent::server::{self, Server};

#[derive(Deserialize)]
struct Fixtures {
    parse: Vec<ParseCase>,
    pipeline: Vec<PipeCase>,
}

#[derive(Deserialize)]
struct ParseCase {
    name: String,
    input: String,
    expect: Vec<ExpectCall>,
}

#[derive(Deserialize)]
struct ExpectCall {
    name: String,
    arguments: Value,
}

#[derive(Deserialize)]
struct PipeCase {
    name: String,
    prompt: String,
    #[serde(default)]
    expect_model_prefix: Option<String>,
    #[serde(default)]
    expect_model_contains: Option<String>,
    #[serde(default)]
    expect_tool: Option<String>,
    #[serde(default)]
    expect_tool_content: Option<String>,
    #[serde(default)]
    expect_final_model: Option<String>,
}

fn fixtures() -> Fixtures {
    serde_json::from_str(include_str!("fixtures/golden.json")).expect("golden.json 格式错误")
}

#[test]
fn golden_parse() {
    let fx = fixtures();
    let mut failed = Vec::new();
    for c in &fx.parse {
        let got = parse::extract(&c.input);
        let ok = got.len() == c.expect.len()
            && got
                .iter()
                .zip(c.expect.iter())
                .all(|(g, e)| g.name == e.name && g.arguments == e.arguments);
        if !ok {
            failed.push(format!(
                "{}: 期望 {:?}，实际 {:?}",
                c.name,
                c.expect.iter().map(|e| &e.name).collect::<Vec<_>>(),
                got
            ));
        }
    }
    assert!(failed.is_empty(), "golden 解析失败:\n{}", failed.join("\n"));
}

fn debug_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let mut cur = exe.as_path();
    while let Some(p) = cur.parent() {
        if p.join("featheragent").is_file() {
            return p.to_path_buf();
        }
        cur = p;
    }
    panic!("找不到 target/debug");
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &dst.join(p.file_name().unwrap()));
        } else {
            std::fs::copy(&p, dst.join(p.file_name().unwrap())).unwrap();
        }
    }
}

fn start() -> (
    Arc<Kernel>,
    Arc<Server>,
    thread::JoinHandle<()>,
    PathBuf,
    client::Connect,
) {
    let root = std::env::temp_dir().join(format!("fa-golden-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let debug = debug_dir();
    for b in ["featheragent", "feather-plugin", "feather-storage"] {
        std::fs::copy(debug.join(b), root.join(b)).unwrap();
    }
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins"),
        &root.join("plugins"),
    );
    let paths = Paths::resolve(
        &root.join("featheragent"),
        &Overrides {
            install_root: Some(root.clone()),
            ..Default::default()
        },
    );
    let kernel = Kernel::new(
        paths.clone(),
        Config {
            sync: "never".into(),
            ..Default::default()
        },
        true,
    )
    .unwrap();
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
    (kernel, srv, h, root, conn)
}

fn wait_finish(conn: &client::Connect, session: &str) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut seen = Vec::new();
    while Instant::now() < deadline {
        match conn.events.recv_timeout(Duration::from_millis(200)) {
            Ok(v) => {
                let kind = v
                    .pointer("/params/kind")
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                let sess = v
                    .pointer("/params/payload/session")
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                if sess == session {
                    seen.push(v.clone());
                    if kind == "chat.finished" {
                        return seen;
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => break,
        }
    }
    panic!("golden 等待 chat.finished 超时");
}

#[test]
fn golden_pipeline_replay() {
    let fx = fixtures();
    let (kernel, srv, handle, root, conn) = start();
    conn.client
        .call("session.create", json!({"id": "golden"}))
        .unwrap();
    let ws = kernel.paths.workspace.join("golden");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("golden.txt"), "golden-content").unwrap();

    for c in &fx.pipeline {
        conn.client
            .call("chat.send", json!({"session": "golden", "text": c.prompt}))
            .unwrap();
        let seen = wait_finish(&conn, "golden");
        let model: Vec<String> = seen
            .iter()
            .filter(|v| v.pointer("/params/kind").and_then(|x| x.as_str()) == Some("model.message"))
            .filter_map(|v| {
                v.pointer("/params/payload/text")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string())
            })
            .collect();
        if let Some(p) = &c.expect_model_prefix {
            assert!(
                model.iter().any(|m| m.starts_with(p)),
                "{}: model={model:?}",
                c.name
            );
        }
        if let Some(k) = &c.expect_model_contains {
            assert!(
                model.iter().any(|m| m.contains(k)),
                "{}: model={model:?}",
                c.name
            );
        }
        if let Some(tool) = &c.expect_tool {
            let hit = seen.iter().any(|v| {
                v.pointer("/params/kind").and_then(|x| x.as_str()) == Some("tool.result")
                    && v.pointer("/params/payload/name").and_then(|x| x.as_str()) == Some(tool)
                    && v.pointer("/params/payload/result/content")
                        .and_then(|x| x.as_str())
                        == c.expect_tool_content.as_deref()
            });
            assert!(hit, "{}: 未找到工具结果", c.name);
        }
        if let Some(m) = &c.expect_final_model {
            assert!(
                model.iter().any(|x| x == m),
                "{}: final model={model:?}",
                c.name
            );
        }
    }
    let snap = conn
        .client
        .call("ctx.inspect", json!({"session": "golden"}))
        .unwrap();
    let tokens = snap
        .pointer("/snapshot/stats/total_tokens")
        .and_then(|x| x.as_u64())
        .unwrap_or(0);
    assert!(tokens <= 8192 * 95 / 100, "golden 会话预算超 95%: {tokens}");

    drop(conn);
    srv.stop();
    let _ = handle.join();
    kernel.stop();
    let _ = std::fs::remove_dir_all(root);
}
