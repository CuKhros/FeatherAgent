//! `featheragent`：内核 CLI（serve / doctor / bench）。
//!
//! 体积目标 ≤ 8 MB 的正是这个二进制；TUI 与 GUI 是独立前端，不参与链接。

use std::io::Write;
use std::path::PathBuf;
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
            println!("featheragent {} (contract {})", featheragent::VERSION, featheragent::proto::CONTRACT_API);
            Ok(())
        }
        "serve" => serve(&args),
        "doctor" => doctor(&args),
        "bench" => bench(&args),
        "gui" => launch_gui(&args),
        "chat" => chat(&args),
        other => {
            eprintln!("未知子命令 {other}；可用: serve | doctor | bench | gui | chat | version | help");
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
    println!("FeatherAgent {} (contract {})", featheragent::VERSION, featheragent::proto::CONTRACT_API);
    println!("用法: featheragent <serve|doctor|bench|gui|chat|version|help> [选项]");
    println!();
    if topic.is_empty() || topic == "help" {
        println!("  serve    启动内核（命名管道/UDS）");
        println!("  doctor   自检：verify-log / --check-storage / --stress-log N");
        println!("  bench    本机体积、启动、RSS、写入、组装、RPC 指标");
        println!("  gui      拉起同目录的 featheragent-gui[.exe]");
        println!("  chat     用内核契约发一条消息并打印事件（S3/脚本用）");
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
    if topic.is_empty() || topic == "gui" {
        println!("gui 选项: --root <dir> / --app（独立浏览器窗口）");
    }
    if topic.is_empty() || topic == "chat" {
        println!("chat 选项: --root <dir> / --session <id> / --text <文本> / --timeout <秒>");
    }
}

/// `featheragent gui`：找到同目录或 plugins/ 下的 GUI 桥并拉起（§8 安装建议）。
fn launch_gui(args: &featheragent::util::Args) -> Result<(), String> {
    let paths = paths_from(args)?;
    let exe = if cfg!(windows) { "featheragent-gui.exe" } else { "featheragent-gui" };
    let candidates = [
        paths.install_root.join(exe),
        paths.install_root.join("featheragent-gui").join(exe),
        paths.install_root.join("plugins").join("featheragent-gui").join(exe),
    ];
    let Some(bin) = candidates.iter().find(|p| p.is_file()) else {
        return Err(format!(
            "未找到 featheragent-gui；请下载 gui 包并解压到 {} 或 {}/plugins/featheragent-gui/",
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
    let status = cmd.status().map_err(|e| format!("启动 GUI 失败: {e}"))?;
    if !status.success() {
        return Err(format!("GUI 退出码 {status}"));
    }
    Ok(())
}

/// `featheragent chat`：不依赖 TUI/GUI 的最小契约客户端，用于脚本 / S3 真机测试。
fn chat(args: &featheragent::util::Args) -> Result<(), String> {
    let paths = paths_from(args)?;
    let socket = args.path("socket").unwrap_or_else(|| paths.socket());
    let session = args.flag("session").unwrap_or("cli");
    let text = args.flag("text").ok_or("chat 需要 --text <文本>")?;
    let timeout = args
        .flag("timeout")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(180);

    let conn = Client::connect(&socket)
        .map_err(|e| format!("连接内核失败（{}）: {e}", socket.display()))?;
    conn.client
        .call_with_timeout("initialize", serde_json::json!({}), Duration::from_secs(3))
        .map_err(|e| format!("initialize 失败: {e}"))?;
    let created = conn
        .client
        .call("session.create", serde_json::json!({"id": session}))
        .map_err(|e| format!("session.create 失败: {e}"))?;
    eprintln!(
        "session {} state={}",
        created.get("id").and_then(|x| x.as_str()).unwrap_or(session),
        created.get("state").and_then(|x| x.as_str()).unwrap_or("?")
    );
    conn.client
        .call("event.subscribe", serde_json::json!({"kinds": []}))
        .map_err(|e| format!("event.subscribe 失败: {e}"))?;
    conn.client
        .call("chat.send", serde_json::json!({"session": session, "text": text}))
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
                let kind = v
                    .get("params")
                    .and_then(|p| p.get("kind"))
                    .and_then(|k| k.as_str());
                if kind == Some("chat.finished") {
                    println!("{}", serde_json::to_string(&v).unwrap_or_default());
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
    let cfg = Config::load(&paths.data.join("config.json"));
    cfg
}

fn serve(args: &featheragent::util::Args) -> Result<(), String> {
    let paths = paths_from(args)?;
    let mut cfg = load_cfg(&paths);
    if let Some(s) = args.flag("sync") {
        cfg.sync = s.to_string();
    }
    if let Some(p) = args.flag("provider") {
        cfg.provider_plugin = p.to_string();
    }
    if let Some(m) = args.flag("model") {
        cfg.model = m.to_string();
    }
    if let Some(b) = args.flag("base-url") {
        cfg.base_url = b.to_string();
    }
    if args.has("--strict-temp") {
        cfg.strict_temp = true;
    }
    let force = args.has("--force");
    let socket = args
        .path("socket")
        .unwrap_or_else(|| paths.socket());

    let kernel = Kernel::new(paths.clone(), cfg.clone(), force)?;
    // 组提交 flusher（§3.1：默认 5 ms 触发 fdatasync）
    {
        let k = kernel.clone();
        let every = 5u64;
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(every));
            let _ = k.store.flush();
        });
    }

    let codec = match args.flag("codec").unwrap_or("msgpack") {
        "json" => Codec::Json,
        "msgpack" | "" => Codec::MsgPack,
        other => return Err(format!("未知 --codec {other}；可用: msgpack | json")),
    };
    let srv: Arc<Server> = Server::new(kernel.clone(), socket.clone(), codec);
    let mut err = std::io::stderr();
    let _ = writeln!(
        err,
        "featheragent {}  socket={}  data={}  provider={}  codec={}",
        featheragent::VERSION,
        socket.display(),
        paths.data.display(),
        cfg.provider_plugin,
        match codec { Codec::Json => "json", Codec::MsgPack => "msgpack" }
    );
    for w in kernel.warnings.read().iter() {
        let _ = writeln!(err, "warn: {w}");
    }

    let r = srv.clone().run();
    kernel.stop();
    let _ = kernel.store.flush();
    r.map_err(|e| e.to_string())
}

fn doctor(args: &featheragent::util::Args) -> Result<(), String> {
    let paths = paths_from(args)?;
    let cfg = load_cfg(&paths);
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
        println!("{}", serde_json::to_string_pretty(&serde_json::Value::Object(out)).unwrap());
        return Ok(());
    }

    if args.has("--stress-log") {
        let n: usize = args.flag("stress-log").and_then(|s| s.parse().ok()).unwrap_or(1000);
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
                .append("s", EventKind::UserMessage, serde_json::json!({"i": j, "pad": "x".repeat(20)}))
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
            let cut = if len == 0 { 0 } else { (rng.next() as usize) % len };
            let f = std::fs::OpenOptions::new().write(true).open(last).map_err(|e| e.to_string())?;
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
    let dir = std::env::temp_dir().join(format!("fa-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    if !json_only {
        eprintln!("bench[1/6] 二进制体积");
    }
    // 1) 二进制体积
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let size_bytes = std::fs::metadata(&exe).map(|m| m.len()).unwrap_or(0);

    if !json_only {
        eprintln!("bench[2/6] 进程内启动 / RSS");
    }
    // 2) 进程内启动到 socket 可连接
    let mut boot_ms = Vec::new();
    let mut rss_kb = 0u64;
    for i in 0..3 {
        let bdir = dir.join(format!("boot{i}"));
        let paths = Paths::resolve(&exe, &Overrides {
            install_root: Some(bdir.clone()),
            ..Default::default()
        });
        let cfg = Config::default();
        let t0 = Instant::now();
        let k = Kernel::new(paths.clone(), cfg, true)?;
        let socket = paths.socket();
        let srv = Server::new(k.clone(), socket.clone(), featheragent::proto::Codec::MsgPack);
        let handle = {
            let s = srv.clone();
            thread::spawn(move || {
                let _ = s.run();
            })
        };
        server::wait_ready(&socket, Duration::from_secs(10)).map_err(|e| e.to_string())?;
        boot_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
        if i == 0 {
            rss_kb = read_rss_kb(std::process::id());
        }
        srv.stop();
        let _ = handle.join();
        k.stop();
        let _ = std::fs::remove_dir_all(&bdir);
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
    let store = LogStore::open(&dir.join("log"), LogConfig {
        sync: SyncMode::Never,
        ..Default::default()
    })
    .map_err(|e| e.to_string())?;
    let mut lat = Vec::new();
    for i in 0..2000 {
        let t = Instant::now();
        store.append("bench", EventKind::UserMessage, serde_json::json!({"i": i})).map_err(|e| e.to_string())?;
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
        events.push(Event::new(i * 2 + 1, i, EventKind::UserMessage, serde_json::json!({"text": format!("问题 {i} {}", "x".repeat(60))})));
        events.push(Event::new(i * 2 + 2, i, EventKind::ModelMessage, serde_json::json!({"text": format!("回答 {i} {}", "y".repeat(60))})));
    }
    let profile = ModelProfile { name: "bench".into(), context_window: 8192, max_output: 512, ..Default::default() };
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
    let paths = Paths::resolve(&exe, &Overrides { install_root: Some(rdir.clone()), ..Default::default() });
    let k = Kernel::new(paths.clone(), Config::default(), true)?;
    let socket = paths.socket();
    let srv = Server::new(k.clone(), socket.clone(), featheragent::proto::Codec::MsgPack);
    let handle = {
        let s = srv.clone();
        thread::spawn(move || {
            let _ = s.run();
        })
    };
    server::wait_ready(&socket, Duration::from_secs(10)).map_err(|e| e.to_string())?;
    let conn = Client::connect(&socket).map_err(|e| e.to_string())?;
    let mut rpc = Vec::new();
    for _ in 0..200 {
        let t = Instant::now();
        conn.client.call("system.info", serde_json::json!({})).map_err(|e| e)?;
        rpc.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    rpc.sort_by(|a, b| a.partial_cmp(b).unwrap());
    drop(conn);
    srv.stop();
    let _ = handle.join();
    k.stop();
    let _ = std::fs::remove_dir_all(&dir);

    let m = serde_json::json!({
        "binary_bytes": size_bytes,
        "binary_mb": size_bytes as f64 / 1_048_576.0,
        "boot_ms_p50": percentile(&boot_ms, 0.5),
        "spawn_to_socket_ms_min": if spawn_ms.is_empty() { 0.0 } else { spawn_ms.iter().cloned().fold(f64::INFINITY, f64::min) },
        "idle_rss_kb": rss_kb,
        "event_write_p50_us": percentile(&lat, 0.5),
        "event_write_p95_us": percentile(&lat, 0.95),
        "ctx_assemble_p50_ms": percentile(&asm_lat, 0.5),
        "ctx_assemble_p95_ms": percentile(&asm_lat, 0.95),
        "rpc_p50_ms": percentile(&rpc, 0.5),
        "rpc_p95_ms": percentile(&rpc, 0.95),
    });
    let gates = serde_json::json!({
        "binary_le_8mb": size_bytes <= 8 * 1024 * 1024,
        "boot_lt_20ms": percentile(&boot_ms, 0.5) < 20.0,
        "rss_le_15mb": rss_kb <= 15 * 1024,
        "write_p95_lt_100us": percentile(&lat, 0.95) < 100.0,
        "assemble_p95_lt_30ms": percentile(&asm_lat, 0.95) < 30.0,
        "rpc_p95_lt_1ms": percentile(&rpc, 0.95) < 1.0,
    });
    if json_only {
        println!("{}", serde_json::to_string(&serde_json::json!({"metrics": m, "gates": gates})).unwrap());
    } else {
        println!("FeatherAgent 内核实测（{}）", featheragent::VERSION);
        println!("  二进制体积      {:.2} MiB   (≤ 8 MiB)", size_bytes as f64 / 1_048_576.0);
        println!("  进程内启动 P50   {:.2} ms   (< 20 ms)", percentile(&boot_ms, 0.5));
        println!("  spawn→socket    {:.2} ms", if spawn_ms.is_empty() { 0.0 } else { spawn_ms.iter().cloned().fold(f64::INFINITY, f64::min) });
        println!("  空闲 RSS        {} KiB   (≤ 15 MiB)", rss_kb);
        println!("  事件写入 P50/P95 {:.1} / {:.1} µs (P95 < 100 µs)", percentile(&lat, 0.5), percentile(&lat, 0.95));
        println!("  上下文组装 P95  {:.2} ms   (< 30 ms)", percentile(&asm_lat, 0.95));
        println!("  RPC 往返 P95    {:.3} ms   (< 1 ms)", percentile(&rpc, 0.95));
        println!("门禁: {}", serde_json::to_string_pretty(&gates).unwrap());
        let _ = human_bytes(size_bytes);
        let _ = std::io::stdout().flush();
    }
    Ok(())
}

#[cfg(windows)]
fn read_rss_kb(pid: u32) -> u64 {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
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
