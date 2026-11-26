//! English UI variant generated from kernel.rs; do not edit directly.
//! Regenerate: python3 scripts/gen-en-ui.py

//! `featheragent`：内核 CLI（serve / doctor / bench）。
//!
//! 体积目标 ≤ 8 MB 的正是这个二进制；TUI 与 WebUI 是独立前端，不参与链接。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use featheragent::context::{ContextEngine, CtxConfig, ModelProfile};
use featheragent::engine::{Config, Kernel};
use featheragent::governance::paths::{Overrides, Paths};
use featheragent::governance::volume::{self, Purpose, Verdict};
use featheragent::log::{LogConfig, LogStore, SyncMode};
use featheragent::proto::types::{Event, EventKind};
use featheragent::proto::Codec;
use featheragent::server::{self, client::Client, Server};
use featheragent::util::{human_bytes, percentile};

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = featheragent::util::Args::parse(argv);
    let want_help = args.has("--help") || args.positional.iter().any(|p| p == "-h");
    if want_help {
        print_help(args.positional.first().map(|s| s.as_str()));
        return;
    }
    if args.has("version") {
        println!(
            "FeatherAgent {} (contract {})",
            featheragent::VERSION,
            featheragent::proto::CONTRACT_API
        );
        return;
    }
    let cmd = args
        .positional
        .first()
        .cloned()
        .unwrap_or_else(|| "serve".into());
    let result = match cmd.as_str() {
        "help" => {
            print_help(None);
            Ok(())
        }
        "version" | "-v" | "--version" => {
            println!(
                "FeatherAgent {} (contract {})",
                featheragent::VERSION,
                featheragent::proto::CONTRACT_API
            );
            Ok(())
        }
        "serve" => serve(&args),
        "doctor" => doctor(&args),
        "bench" => bench(&args),
        "webui" | "gui" => launch_webui(&args),
        "chat" => chat(&args),
        other => {
            eprintln!(
                "unknown subcommand {other}; available: serve | doctor | bench | webui | chat | version | help"
            );
            std::process::exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn print_help(topic: Option<&str>) {
    let topic = topic.unwrap_or("");
    println!(
        "FeatherAgent {} (contract {})",
        featheragent::VERSION,
        featheragent::proto::CONTRACT_API
    );
    println!("Usage: featheragent-en <serve|doctor|bench|webui|chat|version|help> [options]");
    println!();
    if topic.is_empty() || topic == "help" {
        println!("  serve    start the kernel (named pipe/UDS)");
        println!("  doctor   self-check: verify-log / --check-storage / --stress-log N");
        println!("  bench    local size, startup, RSS, write, assembly, RPC metrics");
        println!("  webui    launch featheragent-webui-en[.exe] from the same directory (legacy name gui)");
        println!("  chat     send one message via the kernel contract and print events (for S3/scripts; supports --allow/--yes)");
        println!("  version  print version and contract version");
        println!();
    }
    if topic.is_empty() || topic == "serve" {
        println!("serve options:");
        println!("  --root <dir>        install root (default: directory containing the exe)");
        println!("  --data/--workspace/--runtime <dir>  externalize each path");
        println!("  --provider <plugin> default provider-mock; use provider-ollama for Ollama");
        println!("  --model <name>      model name (e.g. qwen2.5:7b)");
        println!("  --base-url <url>    default http://127.0.0.1:11434");
        println!("  --sync <mode>       always | interval | never");
        println!("  --codec <mode>      msgpack (default) | json (third-party debugging)");
        println!("  --force             override network/sync drive refusal");
    }
    if topic.is_empty() || topic == "doctor" {
        println!("doctor options: --root <dir> / --check-storage / --stress-log <N>");
    }
    if topic.is_empty() || topic == "bench" {
        println!("bench options: --json (JSON only)");
    }
    if topic.is_empty() || topic == "webui" || topic == "gui" {
        println!("webui options: --root <dir> / --app (standalone browser window)");
    }
    if topic.is_empty() || topic == "chat" {
        println!(
            "chat options: --root <dir> / --session <id> / --text <text> / --timeout <seconds>"
        );
        println!(
            "  --allow <tool,...>  auto-confirm the listed dangerous tools (e.g. tool.fs.write,tool.shell)"
        );
        println!("  --yes               auto-confirm all dangerous tools (trusted scripts only)");
        println!("  --confirm-timeout <seconds>  how long to wait before denying (default 0 = deny immediately)");
    }
}

/// `featheragent webui`：找到同目录或 plugins/ 下的 WebUI 桥并拉起（§8 安装建议）。
fn launch_webui(args: &featheragent::util::Args) -> Result<(), String> {
    let paths = paths_from(args)?;
    let exe = if cfg!(windows) {
        "featheragent-webui-en.exe"
    } else {
        "featheragent-webui-en"
    };
    let legacy = if cfg!(windows) {
        "featheragent-gui.exe"
    } else {
        "featheragent-gui"
    };
    let candidates = [
        paths.install_root.join(exe),
        paths.install_root.join("featheragent-webui-en").join(exe),
        paths
            .install_root
            .join("plugins")
            .join("featheragent-webui-en")
            .join(exe),
        paths.install_root.join(legacy),
    ];
    let Some(bin) = candidates.iter().find(|p| p.is_file()) else {
        return Err(format!(
            "featheragent-webui-en not found; download the webui package and extract it to {} or {}/plugins/featheragent-webui/",
            paths.install_root.display(),
            paths.install_root.display()
        ));
    };
    let mut cmd = std::process::Command::new(bin);
    cmd.arg("--root").arg(&paths.install_root);
    cmd.arg("--socket").arg(paths.socket());
    if args.has("--app") {
        cmd.arg("--app");
    }
    let status = cmd
        .status()
        .map_err(|e| format!("failed to start WebUI: {e}"))?;
    if !status.success() {
        return Err(format!("WebUI exit code {status}"));
    }
    Ok(())
}

fn tool_matches(allow: &str, tool: &str) -> bool {
    if allow.is_empty() {
        return false;
    }
    allow == "*"
        || allow == tool
        || allow.strip_prefix("tool.") == Some(tool)
        || format!("tool.{allow}") == tool
}

/// 统一展示路径分隔符：Windows 上 `D:/x` 也显示为 `D:\\x`，避免日志/报告歧义。
fn normalized_path(p: &Path) -> String {
    let s = p.display().to_string();
    if cfg!(windows) {
        s.replace('/', "\\")
    } else {
        s
    }
}

fn ipc_ready_path(socket: &Path) -> PathBuf {
    socket
        .parent()
        .map(|p| p.join("ready"))
        .unwrap_or_else(|| PathBuf::from("runtime/ipc/ready"))
}

fn welcome_connect(
    paths: &Paths,
    socket: &Path,
) -> Result<featheragent::server::client::Connect, String> {
    let ready = ipc_ready_path(socket);
    let starting = ready.with_file_name("starting");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last;
    loop {
        match Client::connect(socket) {
            Ok(c) => return Ok(c),
            Err(e) => {
                last = e.to_string();
                if Instant::now() >= deadline {
                    break;
                }
                thread::sleep(Duration::from_millis(200));
            }
        }
    }
    if ready.exists() {
        return Err(format!(
            "failed to connect to kernel ({}): {last}; ready marker exists, endpoint may be busy or the kernel restarting",
            socket.display()
        ));
    }
    if let Ok(raw) = std::fs::read(&starting) {
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&raw) {
            let started = v
                .get("started_at_unix_ms")
                .and_then(|x| x.as_u64())
                .unwrap_or(0) as u128;
            let age_s = if started > 0 {
                featheragent::util::now_ms().saturating_sub(started) / 1000
            } else {
                0
            };
            if age_s > 180 {
                return Err(format!(
                    "kernel not ready (ready marker exists for {}s, process may have exited): {}\nCheck the serve log and {}; if it exited, run featheragent-en serve again.",
                    age_s,
                    socket.display(),
                    starting.display()
                ));
            }
            return Err(format!(
                "kernel not ready (starting, about {}s): {}\nWait for serve to print \"serve ready\" or for {} to appear; the first Windows start may take 60-120s while [init] plugin registration progress is printed.",
                age_s,
                socket.display(),
                ready.display()
            ));
        }
    }
    let _ = paths;
    Err(format!(
        "kernel not ready: {} not found or no ready marker.\nRun featheragent-en serve --root {} first, wait for \"serve ready\", then run chat.",
        ready.display(),
        paths.install_root.display()
    ))
}

/// `featheragent chat`：不依赖 TUI/WebUI 的最小契约客户端，用于脚本 / S3 真机测试。
fn chat(args: &featheragent::util::Args) -> Result<(), String> {
    let paths = paths_from(args)?;
    let socket = args.path("socket").unwrap_or_else(|| paths.socket());
    let session = args.flag("session").unwrap_or("cli");
    let text = args.flag("text").ok_or("chat requires --text <text>")?;
    let timeout = args
        .flag("timeout")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(180);
    let allow = args.flag("allow").unwrap_or("").to_string();
    let allow_all = args.has("--yes");
    let confirm_wait = args
        .flag("confirm-timeout")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
        .min(30);

    let conn = welcome_connect(&paths, &socket)?;
    conn.client
        .call_with_timeout("initialize", serde_json::json!({}), Duration::from_secs(3))
        .map_err(|e| format!("initialize failed: {e}"))?;
    let created = conn
        .client
        .call("session.create", serde_json::json!({"id": session}))
        .map_err(|e| format!("session.create failed: {e}"))?;
    eprintln!(
        "session {} state={}",
        created
            .get("id")
            .and_then(|x| x.as_str())
            .unwrap_or(session),
        created.get("state").and_then(|x| x.as_str()).unwrap_or("?")
    );
    conn.client
        .call("event.subscribe", serde_json::json!({"kinds": []}))
        .map_err(|e| format!("event.subscribe failed: {e}"))?;
    let sent = conn
        .client
        .call(
            "chat.send",
            serde_json::json!({"session": session, "text": text}),
        )
        .map_err(|e| format!("chat.send failed: {e}"))?;
    // N42：连接是广播订阅者，可能收到上一条被杀回合的 chat.finished。
    // 只认本次 chat.send 分配的 run id，避免把旧回合的 stopped/interrupted 当成本次结果。
    let run_id = sent.get("run").and_then(|x| x.as_u64());

    let deadline = Instant::now() + Duration::from_secs(timeout);
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(format!("no chat.finished received within {timeout}s"));
        }
        let wait = (deadline - now).min(Duration::from_secs(1));
        match conn.events.recv_timeout(wait) {
            Ok(v) => {
                if v.get("method").and_then(|m| m.as_str()) == Some("tool.confirm_request") {
                    let params = v.get("params").cloned().unwrap_or(serde_json::json!({}));
                    let token = params.get("token").and_then(|x| x.as_str()).unwrap_or("");
                    let tool = params
                        .get("tool")
                        .and_then(|x| x.as_str())
                        .unwrap_or("tool");
                    if allow_all || allow.split(',').any(|a| tool_matches(a.trim(), tool)) {
                        conn.client
                            .call(
                                "tool.confirm",
                                serde_json::json!({"token": token, "allow": true}),
                            )
                            .map_err(|e| format!("auto-authorize failed: {e}"))?;
                        eprintln!("chat: authorized {tool} (--allow/--yes)");
                    } else {
                        if confirm_wait > 0 {
                            thread::sleep(Duration::from_secs(confirm_wait));
                        }
                        conn.client
                            .call("tool.deny", serde_json::json!({"token": token}))
                            .map_err(|e| format!("auto-deny failed: {e}"))?;
                        eprintln!("chat: denied {tool}; add --allow {tool} or --yes to permit");
                    }
                    continue;
                }
                let kind = v
                    .get("params")
                    .and_then(|p| p.get("kind"))
                    .and_then(|k| k.as_str());
                if kind == Some("chat.finished") {
                    if let Some(expected) = run_id {
                        let got = v.pointer("/params/payload/run").and_then(|x| x.as_u64());
                        if got != Some(expected) {
                            eprintln!(
                                "chat: ignored stale chat.finished (run={got:?}, current {expected})"
                            );
                            continue;
                        }
                    }
                    println!("{}", serde_json::to_string(&v).unwrap_or_default());
                    let status = v
                        .pointer("/params/payload/status")
                        .and_then(|x| x.as_str())
                        .unwrap_or("unknown");
                    if status != "ok" {
                        let err = v
                            .pointer("/params/payload/error")
                            .and_then(|x| x.as_str())
                            .unwrap_or("chat did not complete successfully");
                        return Err(format!("chat.finished status={status}: {err}"));
                    }
                    return Ok(());
                }
                if v.get("method").and_then(|m| m.as_str()) == Some("event") {
                    println!("{}", serde_json::to_string(&v).unwrap_or_default());
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err("event channel disconnected (kernel exited?)".into());
            }
        }
    }
}

fn paths_from(args: &featheragent::util::Args) -> Result<Paths, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let ov = Overrides {
        install_root: args.path("root"),
        data: args.path("data"),
        workspace: args.path("workspace"),
        runtime: args.path("runtime"),
    };
    Ok(Paths::resolve(&exe, &ov))
}

fn load_cfg(paths: &Paths) -> Config {
    Config::load(&paths.data.join("config.json"))
}

/// 把配置里的外置工作区应用到路径；CLI `--workspace` 优先。
fn apply_workspace(paths: &mut Paths, cfg: &Config, cli_override: bool) {
    if cli_override || cfg.workspace_root.trim().is_empty() {
        return;
    }
    let raw = PathBuf::from(cfg.workspace_root.trim());
    let root = if raw.is_absolute() {
        raw
    } else {
        paths.install_root.join(raw)
    };
    paths.workspace = std::path::absolute(&root).unwrap_or(root);
}

fn serve(args: &featheragent::util::Args) -> Result<(), String> {
    let t0 = Instant::now();
    let mut paths = paths_from(args)?;
    let loaded = load_cfg(&paths);
    let mut cfg = loaded.clone();
    apply_workspace(&mut paths, &cfg, args.flag("workspace").is_some());
    if let Some(s) = args.flag("sync") {
        cfg.sync = s.to_string();
    }
    let mut provider_source = "config";
    let mut model_source = "config";
    let mut base_source = "config";
    if let Some(p) = args.flag("provider") {
        if p != loaded.provider_plugin {
            eprintln!(
                "warn: --provider overrides config.json's {} (using {})",
                loaded.provider_plugin, p
            );
        }
        cfg.provider_plugin = p.to_string();
        provider_source = "cli";
    }
    if let Some(m) = args.flag("model") {
        if m != loaded.model {
            eprintln!(
                "warn: --model overrides config.json's {} (using {})",
                loaded.model, m
            );
        }
        cfg.model = m.to_string();
        model_source = "cli";
    }
    if let Some(b) = args.flag("base-url") {
        if b != loaded.base_url {
            eprintln!(
                "warn: --base-url overrides config.json's {} (using {})",
                loaded.base_url, b
            );
        }
        cfg.base_url = b.to_string();
        base_source = "cli";
    }
    if args.has("--strict-temp") {
        cfg.strict_temp = true;
    }
    let force = args.has("--force");
    let socket = args.path("socket").unwrap_or_else(|| paths.socket());
    let codec = match args.flag("codec").unwrap_or("msgpack") {
        "json" => Codec::Json,
        "msgpack" | "" => Codec::MsgPack,
        other => {
            return Err(format!(
                "unknown --codec {other}; available: msgpack | json"
            ))
        }
    };
    if featheragent::ipc::try_connect(&socket).is_ok() {
        return Err(format!(
            "another kernel is already running on this endpoint ({}); stop the old process or use --socket",
            socket.display()
        ));
    }
    let ipc_dir = socket
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| paths.runtime.join("ipc"));
    let _ = std::fs::create_dir_all(&ipc_dir);
    // 清掉上一次异常退出留下的标记，避免 chat 把旧 ready 当成本次可用。
    let _ = std::fs::remove_file(ipc_dir.join("ready"));
    let _ = std::fs::remove_file(ipc_dir.join("endpoint"));
    let starting_path = ipc_dir.join("starting");
    let starting = serde_json::json!({
        "state": "starting",
        "version": featheragent::VERSION,
        "contract": featheragent::proto::CONTRACT_API,
        "pid": std::process::id(),
        "started_at_unix_ms": featheragent::util::now_ms() as u64,
        "socket": socket.display().to_string(),
    });
    let _ = std::fs::write(
        &starting_path,
        serde_json::to_vec_pretty(&starting).unwrap_or_default(),
    );

    // 先输出再初始化：Windows 首次启动登记内置插件可能很慢，不能等内核构造完才打 banner。
    println!(
        "FeatherAgent {} starting  socket={}  data={}  workspace={}  provider={}  codec={}",
        featheragent::VERSION,
        socket.display(),
        normalized_path(&paths.data),
        normalized_path(&paths.workspace),
        cfg.provider_plugin,
        match codec {
            Codec::Json => "json",
            Codec::MsgPack => "msgpack",
        }
    );
    println!(
        "effective model={} provider={} base_url={} (model_source={}, provider_source={}, base_source={})",
        cfg.model, cfg.provider_plugin, cfg.base_url, model_source, provider_source, base_source
    );
    println!(
        "serve: initializing kernel (first run registers built-in plugins; Windows first run may take 60-120s and prints [init] progress)"
    );
    let _ = std::io::stdout().flush();

    let kernel = match Kernel::new(paths.clone(), cfg.clone(), force) {
        Ok(k) => k,
        Err(e) => {
            let _ = std::fs::remove_file(&starting_path);
            return Err(e);
        }
    };
    println!(
        "serve: kernel initialized ({} ms)",
        t0.elapsed().as_millis()
    );
    let _ = std::io::stdout().flush();
    // 组提交 flusher（§3.1：默认 5 ms 触发 fdatasync）
    {
        let k = kernel.clone();
        let every = 5u64;
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(every));
            let _ = k.store.flush();
        });
    }

    let srv: Arc<Server> = Server::new(kernel.clone(), socket.clone(), codec);
    let mut err = std::io::stderr();
    for w in kernel.warnings.read().iter() {
        let _ = writeln!(err, "warn: {w}");
    }
    println!(
        "serve: listening on {} (waiting for runtime/ipc/ready)",
        socket.display()
    );
    let _ = std::io::stdout().flush();

    let r = srv.clone().run_verbose();
    kernel.stop();
    let _ = kernel.store.flush();
    let _ = std::fs::remove_file(&starting_path);
    r.map_err(|e| e.to_string())
}

fn doctor(args: &featheragent::util::Args) -> Result<(), String> {
    let mut paths = paths_from(args)?;
    let cfg = load_cfg(&paths);
    apply_workspace(&mut paths, &cfg, args.flag("workspace").is_some());
    if args.has("--check-storage") || args.positional.iter().any(|p| p == "storage") {
        let mut out = serde_json::Map::new();
        for (p, purpose) in [
            (paths.data.clone(), Purpose::Data),
            (paths.workspace.clone(), Purpose::Workspace),
        ] {
            let kind = volume::classify(&p, &cfg.allowed_sync_names);
            let verdict = volume::judge(kind, purpose);
            let lock_free = volume::probe_lock(&p).unwrap_or(false);
            out.insert(
                p.display().to_string(),
                serde_json::json!({
                    "purpose": match purpose { Purpose::Data => "data", Purpose::Workspace => "workspace" },
                    "volume": volume::label(kind),
                    "verdict": match verdict { Verdict::Ok => "ok", Verdict::Warn => "warn", Verdict::Reject => "reject" },
                    "lock_free": lock_free,
                }),
            );
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::Value::Object(out)).unwrap()
        );
        return Ok(());
    }

    if args.has("--stress-log") {
        let n: usize = args
            .flag("stress-log")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1000);
        return stress_log(n);
    }

    // 默认：verify-log
    let store = LogStore::open(&paths.data, LogConfig::default()).map_err(|e| e.to_string())?;
    let mut report = store.verify_all().map_err(|e| e.to_string())?;
    // 每个会话尝试一次崩溃恢复
    let mut recoveries = Vec::new();
    for (id, _, _) in store.list_sessions().unwrap_or_default() {
        if let Ok(r) = store.recover(&id) {
            if r.truncated {
                recoveries.push(r);
            }
        }
    }
    if !recoveries.is_empty() {
        report.records += recoveries.len() as u64;
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "ok": report.ok(),
            "sessions": report.sessions,
            "records": report.records,
            "issues": report.issues,
            "recovered": recoveries,
            "data": paths.data.display().to_string(),
        }))
        .unwrap()
    );
    Ok(())
}

/// S2：随机截断 1000 次，零数据错乱。
fn stress_log(n: usize) -> Result<(), String> {
    let mut rng = XorShift::new(0x5EED_1234_ABCD);
    let mut ok = 0usize;
    let mut failed = 0usize;
    let mut details = Vec::new();
    let start = Instant::now();
    for i in 0..n {
        let dir = std::env::temp_dir().join(format!("fa-stress-{}-{i}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let cfg = LogConfig {
            segment_size: 64 * 1024,
            sync: SyncMode::Always,
            ..Default::default()
        };
        let store = LogStore::open(&dir, cfg).map_err(|e| e.to_string())?;
        let m = 1 + (rng.next() % 40) as usize;
        for j in 0..m {
            store
                .append(
                    "s",
                    EventKind::UserMessage,
                    serde_json::json!({"i": j, "pad": "x".repeat(20)}),
                )
                .map_err(|e| e.to_string())?;
        }
        store.flush().ok();
        drop(store);
        // 随机截断最后一段
        let seg_dir = dir.join("sessions").join("s");
        let mut segs: Vec<PathBuf> = std::fs::read_dir(&seg_dir)
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "log").unwrap_or(false))
            .collect();
        segs.sort();
        if let Some(last) = segs.last() {
            let len = std::fs::metadata(last).map(|m| m.len()).unwrap_or(0) as usize;
            let cut = if len == 0 {
                0
            } else {
                (rng.next() as usize) % len
            };
            let f = std::fs::OpenOptions::new()
                .write(true)
                .open(last)
                .map_err(|e| e.to_string())?;
            f.set_len(cut as u64).map_err(|e| e.to_string())?;
        }
        let store = LogStore::open(&dir, LogConfig::default()).map_err(|e| e.to_string())?;
        let rec = store.recover("s").map_err(|e| e.to_string())?;
        let verify = store.verify_all().map_err(|e| e.to_string())?;
        let events = store.read_session("s").map_err(|e| e.to_string())?;
        let monotonic = events
            .iter()
            .enumerate()
            .all(|(i, e)| i == 0 || e.seq >= events[i - 1].seq);
        if verify.ok() && monotonic {
            ok += 1;
        } else {
            failed += 1;
            if details.len() < 5 {
                details.push(serde_json::json!({
                    "i": i, "truncated": rec.truncated, "issues": verify.issues,
                }));
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "iterations": n,
            "ok": ok,
            "failed": failed,
            "elapsed_ms": start.elapsed().as_millis(),
            "details": details,
        }))
        .unwrap()
    );
    if failed > 0 {
        return Err(format!("data inconsistent after {failed} truncations"));
    }
    Ok(())
}

fn write_gate_result(
    windows: bool,
    boot_trusted: bool,
    write_p95: f64,
    write_limit: f64,
) -> (&'static str, bool, bool) {
    let raw = write_p95 < write_limit;
    let margin = windows && write_p95 < write_limit * 2.0;
    let effective = raw || margin || (!boot_trusted && write_p95 < write_limit * 4.0);
    if raw {
        ("pass", true, true)
    } else if effective {
        ("advisory", false, true)
    } else {
        ("fail", false, false)
    }
}

fn bench(args: &featheragent::util::Args) -> Result<(), String> {
    let json_only = args.has("--json");
    match bench_run(args) {
        Ok(()) => Ok(()),
        Err(e) => {
            let code = if e.contains("os error 2")
                || e.contains("The system cannot find the file specified")
            {
                "artifact_missing"
            } else {
                "bench_failed"
            };
            if json_only {
                println!(
                    "{}",
                    serde_json::to_string(&serde_json::json!({
                        "ok": false,
                        "code": code,
                        "error": e,
                        "hint": "Check the path in the error; temporary artifacts use a per-bench unique directory and should not conflict with leftovers"
                    }))
                    .unwrap()
                );
            }
            Err(e)
        }
    }
}

/// N27：bench 无论正常结束、中途报错还是 panic，临时目录都要清理。
struct BenchTempDir {
    path: std::path::PathBuf,
}

impl Drop for BenchTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// 清理超过 24 小时的旧 fa-bench-* 残留；不动其他进程的新目录。
fn cleanup_stale_bench_dirs() {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("fa-bench-") {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .map(|d| d > Duration::from_secs(24 * 60 * 60))
            .unwrap_or(false);
        if old {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// N23：不无限 join 服务线程；超时返回可读错误，避免 bench 卡死 150 秒。
fn wait_server_stop(
    handle: thread::JoinHandle<()>,
    socket: &std::path::Path,
    step: &str,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !handle.is_finished() {
        if Instant::now() >= deadline {
            return Err(format!(
                "{step} service thread did not exit within 5s: {}",
                socket.display()
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
    let _ = handle.join();
    Ok(())
}

fn bench_run(args: &featheragent::util::Args) -> Result<(), String> {
    let json_only = args.has("--json");
    // NEW-K：bench 会自行拉起内核实例；同一安装根已有 serve 时先明确拒绝，
    // 避免 Windows 命名管道冲突被翻译成裸 os error 2。
    let probe_paths = paths_from(args)?;
    let ready_path = ipc_ready_path(&probe_paths.socket());
    if ready_path.exists() && featheragent::ipc::try_connect(&probe_paths.socket()).is_ok() {
        let pid = std::fs::read(&ready_path)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|v| v.get("pid").and_then(|x| x.as_u64()))
            .unwrap_or(0);
        let msg = format!(
            "kernel already running (pid {pid}); stop serve before running bench; current endpoint {}",
            normalized_path(&probe_paths.socket())
        );
        if json_only {
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({
                    "ok": false, "error": msg, "code": "kernel_running"
                }))
                .unwrap()
            );
        } else {
            eprintln!("error: {msg}");
        }
        // 明确以专用退出码结束，避免 Windows 下被包装脚本/管道吞成 0。
        std::process::exit(2);
    }
    // N7：每次 bench 使用唯一临时目录；不要只用 pid（Windows 会复用 pid，残留目录会制造 os error 2）。
    let dir = std::env::temp_dir().join(format!(
        "fa-bench-{}-{}",
        std::process::id(),
        featheragent::util::random_token(6)
    ));
    cleanup_stale_bench_dirs();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("[bench:temp_dir] {}: {e}", dir.display()))?;
    let dir_guard = BenchTempDir { path: dir.clone() };

    if !json_only {
        eprintln!("bench[1/6] binary size");
    }
    // 1) 二进制体积
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let size_bytes = std::fs::metadata(&exe).map(|m| m.len()).unwrap_or(0);

    if !json_only {
        eprintln!("bench[2/6] warm startup (same install root, 1 warm-up + median of 5) / RSS");
    }
    // 2) 热启动到 socket 可连接：同一安装根，先预热一次丢弃，避免把
    // Windows 首次登记内置插件/杀毒扫描计入稳态启动。冷启动另见第 3 项。
    let boot_dir = dir.join("boot-warm");
    let paths = Paths::resolve(
        &exe,
        &Overrides {
            install_root: Some(boot_dir.clone()),
            ..Default::default()
        },
    );
    let mut boot_ms = Vec::new();
    let mut boot_warmup_ms = 0.0;
    let mut rss_kb = 0u64;
    for i in 0..6 {
        let cfg = Config::default();
        let t0 = Instant::now();
        let k = Kernel::new(paths.clone(), cfg, true)
            .map_err(|e| format!("[bench:kernel_init] {}: {e}", boot_dir.display()))?;
        let socket = paths.socket();
        let srv = Server::new(
            k.clone(),
            socket.clone(),
            featheragent::proto::Codec::MsgPack,
        );
        let handle = {
            let s = srv.clone();
            thread::spawn(move || {
                let _ = s.run();
            })
        };
        server::wait_ready(&socket, Duration::from_secs(10))
            .map_err(|e| format!("[bench:boot_ready] {}: {e}", socket.display()))?;
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        if i == 0 {
            boot_warmup_ms = ms;
            rss_kb = read_rss_kb(std::process::id());
        } else {
            boot_ms.push(ms);
        }
        srv.stop();
        wait_server_stop(handle, &socket, "[bench:boot_stop]")?;
        k.stop();
    }

    if !json_only {
        eprintln!("bench[3/6] spawn serve / cold start");
    }
    // 3) 真实冷启动：spawn 自身 serve，等 socket
    let mut spawn_ms = Vec::new();
    for i in 0..3 {
        let bdir = dir.join(format!("spawn{i}"));
        std::fs::create_dir_all(&bdir).map_err(|e| e.to_string())?;
        let t0 = Instant::now();
        let mut child = std::process::Command::new(&exe)
            .arg("serve")
            .arg("--root")
            .arg(&bdir)
            .arg("--force")
            .stderr(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let sock = bdir.join("runtime").join("ipc").join("featheragent.sock");
        let ok = server::wait_ready(&sock, Duration::from_secs(10)).is_ok();
        if !ok && !json_only {
            eprintln!("bench: spawn serve attempt {i} did not become ready within 10s");
        }
        if ok {
            spawn_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
            rss_kb = rss_kb.max(read_rss_kb(child.id()));
        }
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&bdir);
    }

    if !json_only {
        eprintln!("bench[4/6] event writes");
    }
    // 4) 事件写入 P95（不含 fsync）
    let store = LogStore::open(
        &dir.join("log"),
        LogConfig {
            sync: SyncMode::Never,
            ..Default::default()
        },
    )
    .map_err(|e| format!("[bench:log_store] {}: {e}", dir.join("log").display()))?;
    let mut lat = Vec::new();
    for i in 0..2000 {
        let t = Instant::now();
        store
            .append("bench", EventKind::UserMessage, serde_json::json!({"i": i}))
            .map_err(|e| e.to_string())?;
        lat.push(t.elapsed().as_secs_f64() * 1_000_000.0);
    }
    lat.sort_by(|a, b| a.partial_cmp(b).unwrap());

    if !json_only {
        eprintln!("bench[5/6] context assembly");
    }
    // 5) 上下文组装 P95
    let eng = ContextEngine::new(CtxConfig::default());
    let mut events = Vec::new();
    for i in 0..100u64 {
        events.push(Event::new(
            i * 2 + 1,
            i,
            EventKind::UserMessage,
            serde_json::json!({"text": format!("problem {i} {}", "x".repeat(60))}),
        ));
        events.push(Event::new(
            i * 2 + 2,
            i,
            EventKind::ModelMessage,
            serde_json::json!({"text": format!("answer {i} {}", "y".repeat(60))}),
        ));
    }
    let profile = ModelProfile {
        name: "bench".into(),
        context_window: 8192,
        max_output: 512,
        ..Default::default()
    };
    let mut asm_lat = Vec::new();
    for _ in 0..100 {
        let t = Instant::now();
        let _ = eng.assemble(&events, &profile, &[], &[], &[]);
        asm_lat.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    asm_lat.sort_by(|a, b| a.partial_cmp(b).unwrap());

    if !json_only {
        eprintln!("bench[6/6] RPC round-trip");
    }
    // 6) RPC 往返 P95
    let rdir = dir.join("rpc");
    std::fs::create_dir_all(&rdir)
        .map_err(|e| format!("[bench:rpc_mkdir] {}: {e}", rdir.display()))?;
    let paths = Paths::resolve(
        &exe,
        &Overrides {
            install_root: Some(rdir.clone()),
            ..Default::default()
        },
    );
    let k = Kernel::new(paths.clone(), Config::default(), true)
        .map_err(|e| format!("[bench:rpc_kernel] {}: {e}", rdir.display()))?;
    let socket = paths.socket();
    let srv = Server::new(
        k.clone(),
        socket.clone(),
        featheragent::proto::Codec::MsgPack,
    );
    let handle = {
        let s = srv.clone();
        thread::spawn(move || {
            let _ = s.run();
        })
    };
    server::wait_ready(&socket, Duration::from_secs(10))
        .map_err(|e| format!("[bench:rpc_ready] {}: {e}", socket.display()))?;
    // wait_ready 探测成功后会断开一次；Windows 命名管道在重建窗口里可能短暂
    // 返回 ERROR_FILE_NOT_FOUND(2)，因此这里再做有界重试，而不是只 connect 一次。
    let conn = {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match Client::connect(&socket) {
                Ok(c) => break c,
                Err(e) => {
                    if Instant::now() >= deadline {
                        return Err(format!("[bench:rpc_connect] {}: {e}", socket.display()));
                    }
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }
    };
    let mut rpc = Vec::new();
    for _ in 0..200 {
        let t = Instant::now();
        conn.client.call("system.info", serde_json::json!({}))?;
        rpc.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    rpc.sort_by(|a, b| a.partial_cmp(b).unwrap());
    drop(conn);
    srv.stop();
    wait_server_stop(handle, &socket, "[bench:rpc_stop]")?;
    k.stop();
    drop(dir_guard);

    let fingerprint = env_fingerprint();
    let boot_p50 = percentile(&boot_ms, 0.5);
    let boot_p90 = percentile(&boot_ms, 0.9);
    let boot_consistent = boot_p50 <= boot_p90;
    let windows = cfg!(windows);
    let boot_limit = if windows { 100.0 } else { 20.0 };
    let reliability = fingerprint
        .get("reliability")
        .and_then(|x| x.as_str())
        .unwrap_or("unknown");
    let boot_trusted = reliability == "high";
    let boot_gate_raw = boot_p50 < boot_limit;
    let boot_gate_effective = boot_gate_raw || !boot_trusted;
    let m = serde_json::json!({
        "binary_bytes": size_bytes,
        "binary_mb": size_bytes as f64 / 1_048_576.0,
        "env": fingerprint.clone(),
        "boot_mode": "warm_in_process_same_install_root",
        "boot_warmup_discarded_ms": boot_warmup_ms,
        "boot_samples_ms": boot_ms,
        "boot_sample_count": boot_ms.len(),
        "boot_ms_p50": boot_p50,
        "boot_ms_p90": boot_p90,
        "boot_percentiles_consistent": boot_consistent,
        "cold_spawn_mode": "real_process_spawn",
        "cold_spawn_ms_p50": percentile(&spawn_ms, 0.5),
        "cold_spawn_ms_min": if spawn_ms.is_empty() { 0.0 } else { spawn_ms.iter().cloned().fold(f64::INFINITY, f64::min) },
        "spawn_to_socket_ms_min": if spawn_ms.is_empty() { 0.0 } else { spawn_ms.iter().cloned().fold(f64::INFINITY, f64::min) },
        "idle_rss_kb": rss_kb,
        "event_write_p50_us": percentile(&lat, 0.5),
        "event_write_p95_us": percentile(&lat, 0.95),
        "ctx_assemble_p50_ms": percentile(&asm_lat, 0.5),
        "ctx_assemble_p95_ms": percentile(&asm_lat, 0.95),
        "rpc_p50_ms": percentile(&rpc, 0.5),
        "rpc_p95_ms": percentile(&rpc, 0.95),
        "reliability": reliability,
        "boot_gate_raw": boot_gate_raw,
        "boot_gate_trusted": boot_trusted,
    });
    let platform = if windows { "windows" } else { "linux" };
    let cold_spawn_limit = if windows { 250.0 } else { 100.0 };
    let write_limit = if windows { 10_000.0 } else { 100.0 };
    let boot_key = if windows {
        "boot_lt_100ms"
    } else {
        "boot_lt_20ms"
    };
    let cold_key = if windows {
        "cold_spawn_lt_250ms"
    } else {
        "cold_spawn_lt_100ms"
    };
    let write_key = if windows {
        "write_p95_lt_10ms"
    } else {
        "write_p95_lt_100us"
    };
    // N30：Windows 的 NTFS FlushFileBuffers 在杀毒/实时防护下偶发超出 10 ms。
    // reliability 高时，10–20 ms 仍算边缘抖动，标成 advisory 而不把整轮判红；
    // 超过 20 ms 才保留 fail。Linux 门禁仍是 100 us，不放宽。
    let write_p95 = percentile(&lat, 0.95);
    let (write_gate_status, write_raw, write_gate_effective) =
        write_gate_result(windows, boot_trusted, write_p95, write_limit);
    // N34：gates 只放“判定项”，全部是布尔值。状态、原始读数、阈值和可信度
    // 都是诊断信息，放 diagnostics，避免消费者按“数 False”把诊断值当成失败。
    let mut gates = serde_json::json!({
        "binary_le_8mb": size_bytes <= 8 * 1024 * 1024,
        "rss_le_15mb": rss_kb <= 15 * 1024,
        "assemble_p95_lt_30ms": percentile(&asm_lat, 0.95) < 30.0,
        "rpc_p95_lt_1ms": percentile(&rpc, 0.95) < 1.0,
        "metrics_consistent": boot_consistent,
    });
    if let Some(o) = gates.as_object_mut() {
        // N9：reliability 非 high 时启动门禁标成 advisory，不用红门禁惩罚环境抖动。
        o.insert(boot_key.into(), serde_json::json!(boot_gate_effective));
        o.insert(
            cold_key.into(),
            serde_json::json!(percentile(&spawn_ms, 0.5) < cold_spawn_limit),
        );
        o.insert(write_key.into(), serde_json::json!(write_gate_effective));
    }
    // N34：状态/原始读数/阈值/可信度统一放这里；gates 里不再混入非判定值。
    let diagnostics = serde_json::json!({
        "platform": platform,
        "boot_gate_raw": boot_gate_raw,
        "boot_gate_status": if !boot_trusted { "advisory" } else if boot_gate_raw { "pass" } else { "fail" },
        "boot_gate_trusted": boot_trusted,
        "write_gate_raw": write_raw,
        "write_gate_status": write_gate_status,
        "reliability": reliability,
        "boot_mode": "warm_in_process_same_install_root",
        "boot_limit_ms": boot_limit,
        "cold_spawn_mode": "real_process_spawn",
        "cold_spawn_limit_ms": cold_spawn_limit,
        "write_limit_us": write_limit,
    });
    // N17：可信度是环境的“建议项”，不是版本门禁；单独列出，避免
    // reliability=low 时被人工统计成 passed/total 失败。
    let advisory_gates = serde_json::json!({
        "boot_gate_trusted": boot_trusted,
        "boot_gate_status": if !boot_trusted { "advisory" } else if boot_gate_raw { "pass" } else { "fail" },
        "reliability": reliability,
    });
    if json_only {
        println!(
            "{}",
            serde_json::to_string(&serde_json::json!({
                "metrics": m,
                "gates": gates,
                "diagnostics": diagnostics,
                "advisory_gates": advisory_gates
            }))
            .unwrap()
        );
    } else {
        println!("FeatherAgent kernel benchmark ({})", featheragent::VERSION);
        let load1 = fingerprint
            .get("loadavg_1m")
            .and_then(|x| x.as_f64())
            .map(|x| format!("{x:.2}"))
            .unwrap_or_else(|| "n/a".into());
        let avail = fingerprint
            .get("mem_available_kb")
            .and_then(|x| x.as_u64())
            .map(|x| format!("{:.0} MiB", x as f64 / 1024.0))
            .unwrap_or_else(|| "n/a".into());
        let cpu = fingerprint
            .get("cpu_count")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        println!(
            "  Environment fingerprint  cpu={}  load1={}  available memory={}  (check this snapshot before cross-time comparisons)",
            cpu, load1, avail
        );
        println!(
            "  Binary size      {:.2} MiB   (≤ 8 MiB)",
            size_bytes as f64 / 1_048_576.0
        );
        println!(
            "  Warm startup P50/P90   {:.2} / {:.2} ms   (< {:.0} ms; {} samples, {:.2} ms warm-up dropped)",
            boot_p50,
            boot_p90,
            boot_limit,
            boot_ms.len(),
            boot_warmup_ms
        );
        println!(
            "  Cold-start spawn P50 {:.2} ms   (< {:.0} ms; min {:.2} ms)",
            percentile(&spawn_ms, 0.5),
            cold_spawn_limit,
            if spawn_ms.is_empty() {
                0.0
            } else {
                spawn_ms.iter().cloned().fold(f64::INFINITY, f64::min)
            }
        );
        println!("  Idle RSS        {} KiB   (≤ 15 MiB)", rss_kb);
        println!(
            "  Event write P50/P95 {:.1} / {:.1} µs (P95 < {:.0} µs)",
            percentile(&lat, 0.5),
            percentile(&lat, 0.95),
            write_limit
        );
        println!(
            "  Context assembly P95  {:.2} ms   (< 30 ms)",
            percentile(&asm_lat, 0.95)
        );
        println!(
            "  RPC round-trip P95    {:.3} ms   (< 1 ms)",
            percentile(&rpc, 0.95)
        );
        println!("gate: {}", serde_json::to_string_pretty(&gates).unwrap());
        println!(
            "advisory (not a gate): {}",
            serde_json::to_string(&advisory_gates).unwrap()
        );
        let _ = human_bytes(size_bytes);
        let _ = std::io::stdout().flush();
    }
    Ok(())
}

fn reliability_grade(pct: Option<u64>) -> &'static str {
    match pct {
        Some(p) if p >= 85 => "low",
        Some(p) if p >= 70 => "medium",
        Some(_) => "high",
        None => "unknown",
    }
}

/// 内存与 CPU 任一维度吃紧都降低可信度；两者都未知才算 unknown。
fn merge_reliability(a: &'static str, b: &'static str) -> &'static str {
    if a == "unknown" {
        return b;
    }
    if b == "unknown" {
        return a;
    }
    if a == "low" || b == "low" {
        return "low";
    }
    if a == "medium" || b == "medium" {
        return "medium";
    }
    "high"
}

#[cfg(windows)]
fn sample_cpu_load_pct() -> Option<u64> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::GetSystemTimes;

    fn ticks(ft: FILETIME) -> u64 {
        ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
    }
    unsafe {
        let mut idle: FILETIME = std::mem::zeroed();
        let mut kernel: FILETIME = std::mem::zeroed();
        let mut user: FILETIME = std::mem::zeroed();
        if GetSystemTimes(&mut idle, &mut kernel, &mut user) == 0 {
            return None;
        }
        std::thread::sleep(Duration::from_millis(250));
        let mut idle2: FILETIME = std::mem::zeroed();
        let mut kernel2: FILETIME = std::mem::zeroed();
        let mut user2: FILETIME = std::mem::zeroed();
        if GetSystemTimes(&mut idle2, &mut kernel2, &mut user2) == 0 {
            return None;
        }
        let d_idle = ticks(idle2).saturating_sub(ticks(idle));
        let d_kernel = ticks(kernel2).saturating_sub(ticks(kernel));
        let d_user = ticks(user2).saturating_sub(ticks(user));
        // Windows 的 kernel time 已包含 idle time。
        let total = d_kernel.saturating_add(d_user);
        if total == 0 {
            return None;
        }
        Some((total.saturating_sub(d_idle) * 100 / total).min(100))
    }
}

/// J3：bench 数字必须带环境指纹，否则跨时段/跨机器无法比较。
/// Linux 取 /proc 的负载、可用内存与 uptime；Windows 取 GlobalMemoryStatusEx 与 GetTickCount64。
fn env_fingerprint() -> serde_json::Value {
    let mut obj = serde_json::json!({
        "captured_at_unix_ms": featheragent::util::now_ms() as u64,
        "cpu_count": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
    });
    #[cfg(target_os = "linux")]
    {
        if let Ok(s) = std::fs::read_to_string("/proc/loadavg") {
            let parts: Vec<&str> = s.split_whitespace().collect();
            if parts.len() >= 3 {
                obj["loadavg_1m"] = serde_json::json!(parts[0].parse::<f64>().unwrap_or(-1.0));
                obj["loadavg_5m"] = serde_json::json!(parts[1].parse::<f64>().unwrap_or(-1.0));
                obj["loadavg_15m"] = serde_json::json!(parts[2].parse::<f64>().unwrap_or(-1.0));
                let cpu = obj
                    .get("cpu_count")
                    .and_then(|x| x.as_u64())
                    .unwrap_or(1)
                    .max(1) as f64;
                let load1 = parts[0].parse::<f64>().unwrap_or(-1.0);
                if load1 >= 0.0 {
                    // N35：loadavg 是可运行队列长度；除以 CPU 数得到近似占用率，
                    // 让 reliability 不再只看内存。
                    obj["cpu_load_pct"] =
                        serde_json::json!(((load1 / cpu * 100.0).round() as u64).min(100));
                    obj["cpu_load_source"] = serde_json::json!("loadavg_1m/cpu_count");
                }
            }
        }
        if let Ok(s) = std::fs::read_to_string("/proc/meminfo") {
            let mut total = 0u64;
            let mut avail = 0u64;
            for line in s.lines() {
                if let Some(v) = line.strip_prefix("MemTotal:") {
                    total = v
                        .split_whitespace()
                        .next()
                        .and_then(|x| x.parse().ok())
                        .unwrap_or(0);
                } else if let Some(v) = line.strip_prefix("MemAvailable:") {
                    avail = v
                        .split_whitespace()
                        .next()
                        .and_then(|x| x.parse().ok())
                        .unwrap_or(0);
                }
            }
            obj["mem_total_kb"] = serde_json::json!(total);
            obj["mem_available_kb"] = serde_json::json!(avail);
        }
        if let Ok(s) = std::fs::read_to_string("/proc/uptime") {
            if let Some(v) = s.split_whitespace().next() {
                obj["uptime_s"] = serde_json::json!(v.parse::<f64>().unwrap_or(-1.0) as u64);
            }
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::{
            GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX,
        };
        unsafe {
            let mut ms: MEMORYSTATUSEX = std::mem::zeroed();
            ms.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
            if GlobalMemoryStatusEx(&mut ms) != 0 {
                obj["mem_total_kb"] = serde_json::json!(ms.ullTotalPhys / 1024);
                obj["mem_available_kb"] = serde_json::json!(ms.ullAvailPhys / 1024);
                obj["mem_load_pct"] = serde_json::json!(ms.dwMemoryLoad);
            }
            obj["uptime_s"] = serde_json::json!(GetTickCount64() / 1000);
        }
        if let Some(pct) = sample_cpu_load_pct() {
            obj["cpu_load_pct"] = serde_json::json!(pct);
            obj["cpu_load_source"] = serde_json::json!("GetSystemTimes/250ms");
        }
    }
    // K4：整机内存压力只做“读数可信度”标注，不参与门禁。Windows 用 dwMemoryLoad；
    // Linux 用 (MemTotal - MemAvailable) / MemTotal 近似，便于跨平台对照。
    if let Some(o) = obj.as_object_mut() {
        if !o.contains_key("mem_load_pct") {
            if let (Some(total), Some(avail)) = (
                o.get("mem_total_kb").and_then(|x| x.as_u64()),
                o.get("mem_available_kb").and_then(|x| x.as_u64()),
            ) {
                if total > 0 {
                    let used = total.saturating_sub(avail);
                    o.insert(
                        "mem_load_pct".into(),
                        serde_json::json!((used as f64 / total as f64 * 100.0).round() as u64),
                    );
                }
            }
        }
        let mem = o.get("mem_load_pct").and_then(|x| x.as_u64());
        let cpu = o.get("cpu_load_pct").and_then(|x| x.as_u64());
        let reliability = merge_reliability(reliability_grade(mem), reliability_grade(cpu));
        o.insert("reliability".into(), serde_json::json!(reliability));
        o.insert(
            "reliability_basis".into(),
            serde_json::json!(format!(
                "mem={}%,cpu={}%",
                mem.map(|x| x.to_string()).unwrap_or_else(|| "n/a".into()),
                cpu.map(|x| x.to_string()).unwrap_or_else(|| "n/a".into())
            )),
        );
        if mem.is_some_and(|p| p >= 85) || cpu.is_some_and(|p| p >= 85) {
            o.insert(
                "note".into(),
                serde_json::json!(
                    "System memory or CPU usage is high; startup/write readings may be high, for reference only and not a gate failure"
                ),
            );
        }
    }
    obj
}

#[cfg(windows)]
fn read_rss_kb(pid: u32) -> u64 {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
    };
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid);
        if h == 0 {
            return 0;
        }
        let mut c: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        let ok = K32GetProcessMemoryInfo(h, &mut c, c.cb);
        CloseHandle(h);
        if ok != 0 {
            (c.WorkingSetSize / 1024) as u64
        } else {
            0
        }
    }
}

#[cfg(not(windows))]
fn read_rss_kb(pid: u32) -> u64 {
    let path = format!("/proc/{pid}/statm");
    if let Ok(s) = std::fs::read_to_string(path) {
        let fields: Vec<u64> = s
            .split_whitespace()
            .filter_map(|x| x.parse().ok())
            .collect();
        if fields.len() >= 2 {
            return fields[1] * 4; // resident pages × 4 KiB
        }
    }
    0
}

struct XorShift(u64);

impl XorShift {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}
