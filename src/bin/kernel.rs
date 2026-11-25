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
                "未知子命令 {other}；可用: serve | doctor | bench | webui | chat | version | help"
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
    println!("用法: featheragent <serve|doctor|bench|webui|chat|version|help> [选项]");
    println!();
    if topic.is_empty() || topic == "help" {
        println!("  serve    启动内核（命名管道/UDS）");
        println!("  doctor   自检：verify-log / --check-storage / --stress-log N");
        println!("  bench    本机体积、启动、RSS、写入、组装、RPC 指标");
        println!("  webui    拉起同目录的 featheragent-webui[.exe]（兼容旧名 gui）");
        println!("  chat     用内核契约发一条消息并打印事件（S3/脚本用；可 --allow/--yes）");
        println!("  version  打印版本与契约版本");
        println!();
    }
    if topic.is_empty() || topic == "serve" {
        println!("serve 选项:");
        println!("  --root <dir>        安装根（默认 exe 所在目录）");
        println!("  --data/--workspace/--runtime <dir>  分别外置三类路径");
        println!("  --provider <plugin> 默认 provider-mock；Ollama 用 provider-ollama");
        println!("  --model <name>      模型名（如 qwen2.5:7b）");
        println!("  --base-url <url>    默认 http://127.0.0.1:11434");
        println!("  --sync <mode>       always | interval | never");
        println!("  --codec <mode>      msgpack（默认）| json（第三方调试）");
        println!("  --force             覆盖网络盘/同步盘拒绝");
    }
    if topic.is_empty() || topic == "doctor" {
        println!("doctor 选项: --root <dir> / --check-storage / --stress-log <N>");
    }
    if topic.is_empty() || topic == "bench" {
        println!("bench 选项: --json（只输出 JSON）");
    }
    if topic.is_empty() || topic == "webui" || topic == "gui" {
        println!("webui 选项: --root <dir> / --app（独立浏览器窗口）");
    }
    if topic.is_empty() || topic == "chat" {
        println!("chat 选项: --root <dir> / --session <id> / --text <文本> / --timeout <秒>");
        println!(
            "  --allow <tool,...>  自动确认指定 dangerous 工具（如 tool.fs.write,tool.shell）"
        );
        println!("  --yes               自动确认所有 dangerous 工具（仅限受信脚本）");
        println!("  --confirm-timeout <秒> 未放行时等待多久再拒绝（默认 0，立即拒绝）");
    }
}

/// `featheragent webui`：找到同目录或 plugins/ 下的 WebUI 桥并拉起（§8 安装建议）。
fn launch_webui(args: &featheragent::util::Args) -> Result<(), String> {
    let paths = paths_from(args)?;
    let exe = if cfg!(windows) {
        "featheragent-webui.exe"
    } else {
        "featheragent-webui"
    };
    let legacy = if cfg!(windows) {
        "featheragent-gui.exe"
    } else {
        "featheragent-gui"
    };
    let candidates = [
        paths.install_root.join(exe),
        paths.install_root.join("featheragent-webui").join(exe),
        paths
            .install_root
            .join("plugins")
            .join("featheragent-webui")
            .join(exe),
        paths.install_root.join(legacy),
    ];
    let Some(bin) = candidates.iter().find(|p| p.is_file()) else {
        return Err(format!(
            "未找到 featheragent-webui；请下载 webui 包并解压到 {} 或 {}/plugins/featheragent-webui/",
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
    let status = cmd.status().map_err(|e| format!("启动 WebUI 失败: {e}"))?;
    if !status.success() {
        return Err(format!("WebUI 退出码 {status}"));
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
            "连接内核失败（{}）：{last}；ready 标记存在，端点可能繁忙或内核正在重启",
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
                    "内核未就绪（启动标记已存在 {}s，进程可能已退出）：{}\n请检查 serve 日志与 {}；若已退出，重新运行 featheragent serve。",
                    age_s,
                    socket.display(),
                    starting.display()
                ));
            }
            return Err(format!(
                "内核未就绪（启动中，约 {}s）：{}\n请等 serve 输出“serve ready”或 {} 出现后重试；Windows 首次启动可能 60–120s，期间会打印 [init] 插件登记进度。",
                age_s,
                socket.display(),
                ready.display()
            ));
        }
    }
    let _ = paths;
    Err(format!(
        "内核未就绪：未发现 {} 或启动标记。\n请先运行 featheragent serve --root {}，等输出“serve ready”后再执行 chat。",
        ready.display(),
        paths.install_root.display()
    ))
}

/// `featheragent chat`：不依赖 TUI/WebUI 的最小契约客户端，用于脚本 / S3 真机测试。
fn chat(args: &featheragent::util::Args) -> Result<(), String> {
    let paths = paths_from(args)?;
    let socket = args.path("socket").unwrap_or_else(|| paths.socket());
    let session = args.flag("session").unwrap_or("cli");
    let text = args.flag("text").ok_or("chat 需要 --text <文本>")?;
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
        .map_err(|e| format!("initialize 失败: {e}"))?;
    let created = conn
        .client
        .call("session.create", serde_json::json!({"id": session}))
        .map_err(|e| format!("session.create 失败: {e}"))?;
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
        .map_err(|e| format!("event.subscribe 失败: {e}"))?;
    conn.client
        .call(
            "chat.send",
            serde_json::json!({"session": session, "text": text}),
        )
        .map_err(|e| format!("chat.send 失败: {e}"))?;

    let deadline = Instant::now() + Duration::from_secs(timeout);
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(format!("{timeout}s 内未收到 chat.finished"));
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
                            .map_err(|e| format!("自动授权失败: {e}"))?;
                        eprintln!("chat: 已授权 {tool}（--allow/--yes）");
                    } else {
                        if confirm_wait > 0 {
                            thread::sleep(Duration::from_secs(confirm_wait));
                        }
                        conn.client
                            .call("tool.deny", serde_json::json!({"token": token}))
                            .map_err(|e| format!("自动拒绝失败: {e}"))?;
                        eprintln!("chat: 已拒绝 {tool}；需要放行请加 --allow {tool} 或 --yes");
                    }
                    continue;
                }
                let kind = v
                    .get("params")
                    .and_then(|p| p.get("kind"))
                    .and_then(|k| k.as_str());
                if kind == Some("chat.finished") {
                    println!("{}", serde_json::to_string(&v).unwrap_or_default());
                    let status = v
                        .pointer("/params/payload/status")
                        .and_then(|x| x.as_str())
                        .unwrap_or("unknown");
                    if status != "ok" {
                        let err = v
                            .pointer("/params/payload/error")
                            .and_then(|x| x.as_str())
                            .unwrap_or("chat 未成功完成");
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
                return Err("事件通道断开（内核退出？）".into());
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
                "warn: --provider 覆盖 config.json 的 {}（实际使用 {}）",
                loaded.provider_plugin, p
            );
        }
        cfg.provider_plugin = p.to_string();
        provider_source = "cli";
    }
    if let Some(m) = args.flag("model") {
        if m != loaded.model {
            eprintln!(
                "warn: --model 覆盖 config.json 的 {}（实际使用 {}）",
                loaded.model, m
            );
        }
        cfg.model = m.to_string();
        model_source = "cli";
    }
    if let Some(b) = args.flag("base-url") {
        if b != loaded.base_url {
            eprintln!(
                "warn: --base-url 覆盖 config.json 的 {}（实际使用 {}）",
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
        other => return Err(format!("未知 --codec {other}；可用: msgpack | json")),
    };
    if featheragent::ipc::connect(&socket).is_ok() {
        return Err(format!(
            "同一端点已有内核在运行（{}）；请先停止旧进程，或换 --socket",
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
        "serve: 初始化内核（首次启动需登记内置插件，Windows 首次可能 60–120s，会打印 [init] 进度）"
    );
    let _ = std::io::stdout().flush();

    let kernel = match Kernel::new(paths.clone(), cfg.clone(), force) {
        Ok(k) => k,
        Err(e) => {
            let _ = std::fs::remove_file(&starting_path);
            return Err(e);
        }
    };
    println!("serve: 内核初始化完成（{} ms）", t0.elapsed().as_millis());
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
        "serve: 开始监听 {}（等待 runtime/ipc/ready）",
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
        return Err(format!("{failed} 次截断后数据不一致"));
    }
    Ok(())
}

fn bench(args: &featheragent::util::Args) -> Result<(), String> {
    let json_only = args.has("--json");
    match bench_run(args) {
        Ok(()) => Ok(()),
        Err(e) => {
            let code = if e.contains("os error 2") || e.contains("系统找不到指定的文件") {
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
                        "hint": "检查错误里的路径；临时产物按每次 bench 的唯一目录创建，不应与残留冲突"
                    }))
                    .unwrap()
                );
            }
            Err(e)
        }
    }
}

fn bench_run(args: &featheragent::util::Args) -> Result<(), String> {
    let json_only = args.has("--json");
    // NEW-K：bench 会自行拉起内核实例；同一安装根已有 serve 时先明确拒绝，
    // 避免 Windows 命名管道冲突被翻译成裸 os error 2。
    let probe_paths = paths_from(args)?;
    let ready_path = ipc_ready_path(&probe_paths.socket());
    if ready_path.exists() && featheragent::ipc::connect(&probe_paths.socket()).is_ok() {
        let pid = std::fs::read(&ready_path)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .and_then(|v| v.get("pid").and_then(|x| x.as_u64()))
            .unwrap_or(0);
        let msg = format!(
            "检测到内核正在运行（pid {pid}），请先停止 serve 再运行 bench；当前端点 {}",
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
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("[bench:temp_dir] {}: {e}", dir.display()))?;

    if !json_only {
        eprintln!("bench[1/6] 二进制体积");
    }
    // 1) 二进制体积
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let size_bytes = std::fs::metadata(&exe).map(|m| m.len()).unwrap_or(0);

    if !json_only {
        eprintln!("bench[2/6] 热启动（同一安装根，1 次预热 + 5 次中位） / RSS");
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
        let _ = handle.join();
        k.stop();
    }

    if !json_only {
        eprintln!("bench[3/6] spawn serve / 冷启动");
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
            eprintln!("bench: spawn serve 第 {i} 次未在 10s 内就绪");
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
        eprintln!("bench[4/6] 事件写入");
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
        eprintln!("bench[5/6] 上下文组装");
    }
    // 5) 上下文组装 P95
    let eng = ContextEngine::new(CtxConfig::default());
    let mut events = Vec::new();
    for i in 0..100u64 {
        events.push(Event::new(
            i * 2 + 1,
            i,
            EventKind::UserMessage,
            serde_json::json!({"text": format!("问题 {i} {}", "x".repeat(60))}),
        ));
        events.push(Event::new(
            i * 2 + 2,
            i,
            EventKind::ModelMessage,
            serde_json::json!({"text": format!("回答 {i} {}", "y".repeat(60))}),
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
        eprintln!("bench[6/6] RPC 往返");
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
    let _ = handle.join();
    k.stop();
    let _ = std::fs::remove_dir_all(&dir);

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
    let mut gates = serde_json::json!({
        "platform": platform,
        "binary_le_8mb": size_bytes <= 8 * 1024 * 1024,
        "rss_le_15mb": rss_kb <= 15 * 1024,
        "assemble_p95_lt_30ms": percentile(&asm_lat, 0.95) < 30.0,
        "rpc_p95_lt_1ms": percentile(&rpc, 0.95) < 1.0,
        "metrics_consistent": boot_consistent,
        "boot_gate_status": if !boot_trusted { "advisory" } else if boot_gate_raw { "pass" } else { "fail" },
        "boot_gate_raw": boot_gate_raw,
        "reliability": reliability,
        "boot_mode": "warm_in_process_same_install_root",
        "boot_limit_ms": boot_limit,
        "cold_spawn_mode": "real_process_spawn",
        "cold_spawn_limit_ms": cold_spawn_limit,
        "write_limit_us": write_limit,
    });
    if let Some(o) = gates.as_object_mut() {
        // N9：reliability 非 high 时启动门禁标成 advisory，不用红门禁惩罚环境抖动。
        o.insert(boot_key.into(), serde_json::json!(boot_gate_effective));
        o.insert(
            cold_key.into(),
            serde_json::json!(percentile(&spawn_ms, 0.5) < cold_spawn_limit),
        );
        o.insert(
            write_key.into(),
            serde_json::json!(percentile(&lat, 0.95) < write_limit),
        );
    }
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
                "advisory_gates": advisory_gates
            }))
            .unwrap()
        );
    } else {
        println!("FeatherAgent 内核实测（{}）", featheragent::VERSION);
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
            "  环境指纹        cpu={}  load1={}  可用内存={}  （跨时段对比前先看此快照）",
            cpu, load1, avail
        );
        println!(
            "  二进制体积      {:.2} MiB   (≤ 8 MiB)",
            size_bytes as f64 / 1_048_576.0
        );
        println!(
            "  热启动 P50/P90   {:.2} / {:.2} ms   (< {:.0} ms；{} 次样本，预热 {:.2} ms 已丢弃)",
            boot_p50,
            boot_p90,
            boot_limit,
            boot_ms.len(),
            boot_warmup_ms
        );
        println!(
            "  冷启动 spawn P50 {:.2} ms   (< {:.0} ms；min {:.2} ms)",
            percentile(&spawn_ms, 0.5),
            cold_spawn_limit,
            if spawn_ms.is_empty() {
                0.0
            } else {
                spawn_ms.iter().cloned().fold(f64::INFINITY, f64::min)
            }
        );
        println!("  空闲 RSS        {} KiB   (≤ 15 MiB)", rss_kb);
        println!(
            "  事件写入 P50/P95 {:.1} / {:.1} µs (P95 < {:.0} µs)",
            percentile(&lat, 0.5),
            percentile(&lat, 0.95),
            write_limit
        );
        println!(
            "  上下文组装 P95  {:.2} ms   (< 30 ms)",
            percentile(&asm_lat, 0.95)
        );
        println!(
            "  RPC 往返 P95    {:.3} ms   (< 1 ms)",
            percentile(&rpc, 0.95)
        );
        println!("门禁: {}", serde_json::to_string_pretty(&gates).unwrap());
        println!(
            "建议项（不计入门禁）: {}",
            serde_json::to_string(&advisory_gates).unwrap()
        );
        let _ = human_bytes(size_bytes);
        let _ = std::io::stdout().flush();
    }
    Ok(())
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
        let load = o.get("mem_load_pct").and_then(|x| x.as_u64());
        let reliability = match load {
            Some(p) if p >= 85 => "low",
            Some(p) if p >= 70 => "medium",
            Some(_) => "high",
            None => "unknown",
        };
        o.insert("reliability".into(), serde_json::json!(reliability));
        if let Some(p) = load {
            if p >= 85 {
                o.insert(
                    "note".into(),
                    serde_json::json!(format!(
                        "整机内存占用 {p}%，启动/写入读数可能偏高，仅作参考、不作门禁失败"
                    )),
                );
            }
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

#[cfg(test)]
mod tests {
    use super::{apply_workspace, tool_matches};

    #[test]
    fn chat_allow_matches_full_short_and_wildcard() {
        assert!(tool_matches("tool.fs.write", "tool.fs.write"));
        assert!(tool_matches("fs.write", "tool.fs.write"));
        assert!(tool_matches("*", "tool.shell"));
        assert!(!tool_matches("", "tool.fs.read"));
        assert!(!tool_matches("tool.fs.read", "tool.fs.write"));
    }

    #[test]
    fn workspace_root_absolute_and_relative_resolve() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("install");
        std::fs::create_dir_all(&root).unwrap();
        let mut paths = featheragent::governance::paths::Paths {
            install_root: root.clone(),
            data: root.join("data"),
            workspace: root.join("workspace"),
            runtime: root.join("runtime"),
        };
        let mut cfg = featheragent::engine::Config {
            workspace_root: "outer-ws".into(),
            ..Default::default()
        };
        apply_workspace(&mut paths, &cfg, false);
        assert_eq!(paths.workspace, root.join("outer-ws"));

        let abs = d.path().join("abs-ws");
        cfg.workspace_root = abs.to_string_lossy().into_owned();
        apply_workspace(&mut paths, &cfg, false);
        assert_eq!(paths.workspace, abs);

        // CLI --workspace 优先：apply_workspace 不应覆盖
        let before = paths.workspace.clone();
        apply_workspace(&mut paths, &cfg, true);
        assert_eq!(paths.workspace, before);
    }
}
