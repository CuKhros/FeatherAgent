//! 端到端集成测试：真实拉起内核 + 插件进程，走 UDS 契约。
//!
//! 覆盖设计文档门禁：
//! - Phase 1：30 轮稳定对话、崩溃不中止
//! - Phase 2：100 轮长会话预算稳定（demo 用 mock provider，S3 真实模型数字另测）
//! - Phase 3：工具执行、协作式权限门、取消传播、fork/replay、存储索引

use std::path::{Path, PathBuf};
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
        .last()
        .unwrap_or_default()
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
    assert!(info["version"].as_str().unwrap().starts_with("1.1"));
    assert_eq!(info["gate"], "collaborative");
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
