//! 前端契约服务（设计文档 §7.2）。
//!
//! 传输分层是硬约束：**内核只提供 UDS / 命名管道**；HTTP/WS 归 WebUI 包的
//! `featheragent-webui` 桥接器，内核不实现任何 HTTP 服务（否则 8 MB 预算破功）。
//!
//! 最小方法集见文档 §7.2；帧为长度前缀 msgpack（v1 可退 JSON）。

pub mod client;

use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::{json, Value};

use crate::engine::{Kernel, SessionState};
use crate::proto::rpc::{self, Request};
use crate::proto::types::{EventKind, Role};
use crate::proto::{read_frame, write_frame, Codec};

pub struct Server {
    pub kernel: Arc<Kernel>,
    pub socket: PathBuf,
    pub codec: Codec,
    stop: Arc<AtomicBool>,
}

impl Server {
    pub fn new(kernel: Arc<Kernel>, socket: PathBuf, codec: Codec) -> Arc<Self> {
        Arc::new(Self {
            kernel,
            socket,
            codec,
            stop: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        // 触发一次连接，唤醒阻塞中的 accept（Unix: connect UDS；Windows: 连命名管道）。
        // Windows 上实例可能尚未就绪，重试几次；连接本身很快断开是刻意的，
        // accept 侧会把 ERROR_NO_DATA 交回外层检查 stop，不会永久卡住。
        for _ in 0..5 {
            if let Ok(s) = crate::ipc::connect(&self.socket) {
                drop(s);
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// 阻塞运行；返回即表示停止。库/bench 使用，不向 stdout 打印。
    pub fn run(self: Arc<Self>) -> std::io::Result<()> {
        self.run_impl(false)
    }

    /// `serve` CLI 使用：额外把就绪信息打印到 stdout（bench 的 JSON 输出不受污染）。
    pub fn run_verbose(self: Arc<Self>) -> std::io::Result<()> {
        self.run_impl(true)
    }

    fn run_impl(self: Arc<Self>, announce: bool) -> std::io::Result<()> {
        let mut listener = crate::ipc::Listener::bind(&self.socket)?;
        // 把实际端点落盘，第三方前端无需自行复刻命名管道哈希；
        // ready 是“内核已开始监听”的显式标记，前端可据此区分启动中与真失败。
        if let Some(dir) = self.socket.parent() {
            let _ = std::fs::write(dir.join("endpoint"), crate::ipc::endpoint(&self.socket));
            let ready = json!({
                "state": "ready",
                "version": crate::VERSION,
                "contract": crate::proto::CONTRACT_API,
                "endpoint": crate::ipc::endpoint(&self.socket),
                "pid": std::process::id(),
                "ready_at_unix_ms": crate::util::now_ms() as u64,
            });
            let _ = std::fs::write(
                dir.join("ready"),
                serde_json::to_vec_pretty(&ready).unwrap_or_default(),
            );
            let _ = std::fs::remove_file(dir.join("starting"));
            if announce {
                // 显式 flush：Windows 下 stdout 重定向到日志文件时不能等进程退出。
                let mut out = std::io::stdout();
                let _ = writeln!(
                    out,
                    "serve ready: {}\n  endpoint={}\n  runtime={}",
                    crate::VERSION,
                    crate::ipc::endpoint(&self.socket),
                    dir.display()
                );
                let _ = out.flush();
            }
        }
        loop {
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            match listener.accept() {
                Ok(stream) => {
                    if self.stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let k = self.kernel.clone();
                    let codec = self.codec;
                    thread::spawn(move || {
                        if let Err(e) = handle_conn(k, stream, codec) {
                            eprintln!("conn error: {e}");
                        }
                    });
                }
                Err(e) => {
                    if self.stop.load(Ordering::SeqCst) {
                        break;
                    }
                    // Windows 管道收尾竞争会产生 ERROR_NO_DATA(232)/ERROR_BROKEN_PIPE(109)；
                    // 这是正常的 stop/断连唤醒，不要作为错误刷屏。
                    #[cfg(windows)]
                    if matches!(e.raw_os_error(), Some(232) | Some(109)) {
                        thread::sleep(Duration::from_millis(20));
                        continue;
                    }
                    eprintln!("accept error: {e}");
                    thread::sleep(Duration::from_millis(20));
                }
            }
        }
        #[cfg(unix)]
        {
            let _ = std::fs::remove_file(&self.socket);
        }
        if let Some(dir) = self.socket.parent() {
            let _ = std::fs::remove_file(dir.join("ready"));
            let _ = std::fs::remove_file(dir.join("endpoint"));
        }
        Ok(())
    }
}

/// 客户端断开即视为取消：释放该连接持有的会话写锁，并停止仍在跑的模型请求。
/// 否则被 kill 的客户端会留下孤儿回合继续烧算力，且 holder 会挡住后续连接。
fn cancel_connection(kernel: &Arc<Kernel>, conn_id: u64) {
    let owned: Vec<Arc<crate::engine::Session>> = kernel
        .sessions
        .read()
        .values()
        .filter(|s| *s.holder.read() == Some(conn_id))
        .cloned()
        .collect();
    for s in owned {
        let running = matches!(
            *s.state.read(),
            crate::engine::SessionState::Assembling
                | crate::engine::SessionState::Generating
                | crate::engine::SessionState::ToolExecuting
        );
        if running {
            s.cancel.store(true, Ordering::SeqCst);
            let active = s.active_req.lock().clone();
            if let Some((plugin, req)) = active {
                if let Ok(p) = kernel.plugins.get(&plugin) {
                    p.cancel(req);
                }
            }
            kernel.emit(
                &s.id,
                EventKind::ToolCancelled,
                json!({"name": "client.disconnect", "reason": "client_disconnect"}),
            );
        }
        *s.holder.write() = None;
    }
}

fn handle_conn(
    kernel: Arc<Kernel>,
    stream: crate::ipc::Stream,
    codec: Codec,
) -> std::io::Result<()> {
    let conn_id = kernel.next_conn_id();
    let (tx, rx): (Sender<Value>, Receiver<Value>) = mpsc::channel();
    kernel.broadcasts.write().push((conn_id, tx.clone()));

    // writer thread
    let mut wstream = stream.try_clone()?;
    let writer = thread::spawn(move || {
        while let Ok(v) = rx.recv() {
            if write_frame(&mut wstream, &v, codec).is_err() {
                break;
            }
        }
    });

    // bus poller
    let sub = kernel.bus.subscribe(None);
    let sub = Arc::new(Mutex::new(sub));
    let kinds = Arc::new(Mutex::new(Vec::<EventKind>::new()));
    let stop_poll = Arc::new(AtomicBool::new(false));
    let poller = {
        let tx = tx.clone();
        let bus = kernel.bus.clone();
        let sub = sub.clone();
        let kinds = kinds.clone();
        let stop = stop_poll.clone();
        thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let cursor = sub.lock().cursor;
                bus.wait(cursor, Duration::from_millis(100));
                let out = bus.poll(&mut sub.lock());
                let filter = kinds.lock().clone();
                for ev in out.events {
                    if !filter.is_empty() && !filter.contains(&ev.kind) {
                        continue;
                    }
                    let v = rpc::notification(
                        "event",
                        json!({
                            "seq": ev.seq,
                            "ts": ev.ts_wall_ns,
                            "kind": ev.kind.as_str(),
                            "payload": ev.payload,
                        }),
                    );
                    if tx.send(v).is_err() {
                        return;
                    }
                }
                if out.lagged > 0 {
                    let _ = tx.send(rpc::notification(
                        "event.lagged",
                        json!({"dropped": out.lagged}),
                    ));
                }
            }
        })
    };

    // reader loop（同步处理请求）
    let reader = {
        let tx = tx.clone();
        let kernel = kernel.clone();
        thread::spawn(move || -> std::io::Result<()> {
            let mut r = BufReader::new(stream);
            loop {
                match read_frame(&mut r, codec) {
                    Ok(Some(v)) => {
                        let Some(req) = Request::parse(&v) else {
                            continue;
                        };
                        if req.is_notification() {
                            continue;
                        }
                        let id = req.id.unwrap();
                        let resp = match dispatch(
                            &kernel,
                            conn_id,
                            &kinds,
                            codec,
                            &req.method,
                            &req.params,
                        ) {
                            Ok(result) => rpc::success(id, result),
                            Err((code, msg)) => rpc::error(Some(id), code, msg),
                        };
                        if tx.send(resp).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
            Ok(())
        })
    };

    let _ = reader.join();
    cancel_connection(&kernel, conn_id);
    stop_poll.store(true, Ordering::Relaxed);
    let _ = poller.join();
    kernel.broadcasts.write().retain(|(id, _)| *id != conn_id);
    let _ = tx.send(json!({"jsonrpc":"2.0","method":"bye"}));
    let _ = writer.join();
    Ok(())
}

type RpcResult = Result<Value, (i64, String)>;

fn invalid(msg: impl Into<String>) -> (i64, String) {
    (rpc::ERR_INVALID_PARAMS, msg.into())
}

fn dispatch(
    kernel: &Arc<Kernel>,
    conn_id: u64,
    kinds: &Arc<Mutex<Vec<EventKind>>>,
    codec: Codec,
    method: &str,
    params: &Value,
) -> RpcResult {
    match method {
        "initialize" => Ok(json!({
            "version": crate::VERSION,
            "contract": crate::proto::CONTRACT_API,
            "transport": if cfg!(windows) { "pipe" } else { "uds" },
            "codec": match codec { Codec::MsgPack => "msgpack", Codec::Json => "json" },
            "paths": {
                "install_root": kernel.paths.install_root,
                "data": kernel.paths.data,
                "workspace": kernel.paths.workspace,
                "runtime": kernel.paths.runtime,
            },
        })),
        "system.info" => Ok(json!({
            "version": crate::VERSION,
            "uptime_ms": kernel.started.elapsed().as_millis(),
            "sessions": kernel.sessions.read().len(),
            "warnings": kernel.warnings.read().clone(),
            "gate": "collaborative",
            "plugins": kernel.plugins.list().len(),
        })),
        "session.create" => {
            let id = params
                .get("id")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("s-{}", crate::util::random_token(4)));
            let s = kernel.session(&id);
            *s.holder.write() = Some(conn_id);
            kernel.emit(&id, EventKind::SessionCreated, json!({"conn": conn_id}));
            Ok(s.info())
        }
        "session.list" => {
            let log_sessions = kernel.store.list_sessions().unwrap_or_default();
            let live: HashMap<String, Value> = kernel
                .sessions
                .read()
                .iter()
                .map(|(k, v)| (k.clone(), v.info()))
                .collect();
            let mut list: Vec<Value> = log_sessions
                .iter()
                .map(|(id, n, ts)| {
                    json!({"id": id, "records": n, "last_ts": ts, "live": live.get(id)})
                })
                .collect();
            for (id, info) in live {
                if !list.iter().any(|x| x["id"] == json!(id)) {
                    list.push(json!({"id": id, "records": 0, "last_ts": 0, "live": info}));
                }
            }
            Ok(json!({"sessions": list}))
        }
        "session.resume" => {
            let id = params
                .get("session")
                .and_then(|x| x.as_str())
                .ok_or_else(|| invalid("session required"))?;
            let s = kernel.session(id);
            let role = {
                let mut h = s.holder.write();
                match *h {
                    None => {
                        *h = Some(conn_id);
                        Role::Holder
                    }
                    Some(owner) if owner == conn_id => Role::Holder,
                    Some(_) => Role::Observer,
                }
            };
            kernel.emit(
                id,
                EventKind::SessionResumed,
                json!({"conn": conn_id, "role": role}),
            );
            let events = kernel.store.read_session(id).unwrap_or_default();
            let tail: Vec<Value> = events
                .iter()
                .rev()
                .take(200)
                .rev()
                .map(|e| json!({"seq": e.seq, "kind": e.kind.as_str(), "ts": e.ts_wall_ns, "payload": e.payload}))
                .collect();
            Ok(json!({
                "session": id,
                "role": role,
                "state": s.state.read().as_str(),
                "snapshot": s.snapshot.read().clone(),
                "events": tail,
                "total": events.len(),
            }))
        }
        "session.fork" => {
            let id = params
                .get("session")
                .and_then(|x| x.as_str())
                .ok_or_else(|| invalid("session required"))?;
            let as_id = params.get("as").and_then(|x| x.as_str()).unwrap_or("fork");
            let n = kernel
                .store
                .fork(id, as_id)
                .map_err(|e| (rpc::ERR_INTERNAL, e.to_string()))?;
            kernel.session(as_id);
            Ok(json!({"session": as_id, "copied": n}))
        }
        "chat.send" => {
            let id = params
                .get("session")
                .and_then(|x| x.as_str())
                .ok_or_else(|| invalid("session required"))?;
            let text = params
                .get("text")
                .and_then(|x| x.as_str())
                .ok_or_else(|| invalid("text required"))?;
            let s = kernel.session(id);
            let holder = *s.holder.read();
            match holder {
                Some(h) if h != conn_id => {
                    return Err((
                        rpc::ERR_OBSERVER_READONLY,
                        "当前连接是观察视图（单写者模型）".into(),
                    ))
                }
                None => {
                    *s.holder.write() = Some(conn_id);
                }
                _ => {}
            }
            if matches!(
                *s.state.read(),
                SessionState::Assembling | SessionState::Generating | SessionState::ToolExecuting
            ) {
                return Err((rpc::ERR_OCCUPIED, "会话正忙".into()));
            }
            *s.state.write() = SessionState::Idle;
            let k = kernel.clone();
            let ss = s.clone();
            let t = text.to_string();
            thread::spawn(move || {
                if let Err(e) = k.run_turn(&ss, t) {
                    *ss.state.write() = SessionState::Failed;
                    *ss.error.write() = Some(e.clone());
                    k.emit(
                        &ss.id,
                        EventKind::ChatFinished,
                        json!({"status": "failed", "error": e}),
                    );
                }
            });
            Ok(json!({"session": id, "accepted": true, "turn": s.turn.load(Ordering::Relaxed)}))
        }
        "chat.cancel" => {
            let id = params
                .get("session")
                .and_then(|x| x.as_str())
                .ok_or_else(|| invalid("session required"))?;
            let s = kernel.session(id);
            s.cancel.store(true, Ordering::SeqCst);
            let active = s.active_req.lock().clone();
            if let Some((plugin, req)) = active {
                if let Ok(p) = kernel.plugins.get(&plugin) {
                    p.cancel(req);
                }
            }
            kernel.emit(
                id,
                EventKind::ToolCancelled,
                json!({"reason": "chat.cancel"}),
            );
            Ok(json!({"ok": true}))
        }
        "event.subscribe" => {
            let ks = params
                .get("kinds")
                .and_then(|x| x.as_array())
                .cloned()
                .unwrap_or_default();
            let mut out = Vec::new();
            for k in ks {
                if let Some(s) = k.as_str() {
                    if let Some(kind) = EventKind::parse(s) {
                        out.push(kind);
                    }
                }
            }
            *kinds.lock() = out;
            Ok(json!({"ok": true}))
        }
        "tool.confirm" | "tool.deny" => {
            let token = params
                .get("token")
                .and_then(|x| x.as_str())
                .ok_or_else(|| invalid("token required"))?;
            let allow = method == "tool.confirm"
                && params
                    .get("allow")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(true);
            Ok(json!({"ok": kernel.resolve_confirm(token, allow)}))
        }
        "ctx.inspect" => {
            let id = params
                .get("session")
                .and_then(|x| x.as_str())
                .ok_or_else(|| invalid("session required"))?;
            let s = kernel.session(id);
            // 诊断接口绝不阻塞：若会话锁被长回合短暂占用，返回 busy 而不是挂住前端。
            let state = s
                .state
                .try_read()
                .map(|g| g.as_str().to_string())
                .unwrap_or_else(|| "busy".into());
            let snapshot = s
                .snapshot
                .try_read()
                .map(|g| g.clone())
                .unwrap_or(Value::Null);
            Ok(json!({
                "session": id,
                "state": state,
                "snapshot": snapshot,
                "turn": s.turn.load(Ordering::Relaxed),
            }))
        }
        "config.get" => {
            let cfg = kernel.config_json();
            match params.get("path").and_then(|x| x.as_str()) {
                Some(p) => {
                    let pointer = if p.starts_with('/') {
                        p.to_string()
                    } else {
                        format!("/{}", p.split('.').collect::<Vec<_>>().join("/"))
                    };
                    Ok(json!({
                        "value": cfg.pointer(&pointer).cloned().unwrap_or(Value::Null),
                        "config": cfg
                    }))
                }
                None => Ok(cfg),
            }
        }
        "config.set" => {
            if let Some(path) = params.get("path").and_then(|x| x.as_str()) {
                let value = params
                    .get("value")
                    .cloned()
                    .ok_or_else(|| invalid("config.set 的 path 与 value 必须同时提供"))?;
                let cfg = kernel
                    .set_config_path(path, value)
                    .map_err(|e| (rpc::ERR_INVALID_PARAMS, e))?;
                return Ok(json!({"config": cfg}));
            }
            let patch = params
                .get("patch")
                .cloned()
                .or_else(|| params.get("value").cloned())
                .ok_or_else(|| invalid("config.set 需要 patch，或 path + value"))?;
            if !patch.is_object() {
                return Err(invalid(
                    "config.set 的 patch 必须是对象；单字段请用 path + value",
                ));
            }
            let cfg = kernel
                .set_config(&patch)
                .map_err(|e| (rpc::ERR_INVALID_PARAMS, e))?;
            Ok(json!({"config": cfg}))
        }
        "config.replace" => {
            let full = params
                .get("config")
                .cloned()
                .or_else(|| params.get("value").cloned())
                .ok_or_else(|| invalid("config.replace 需要完整 config 对象"))?;
            let cfg = kernel
                .replace_config(&full)
                .map_err(|e| (rpc::ERR_INVALID_PARAMS, e))?;
            Ok(json!({"config": cfg}))
        }
        "storage.stats" => Ok(kernel.storage_stats()),
        "storage.gc" => {
            let dry = params
                .get("dry_run")
                .and_then(|x| x.as_bool())
                .unwrap_or(true);
            Ok(kernel.storage_gc(dry))
        }
        "provider.models" => {
            let cfg = kernel.config_json();
            let provider = params
                .get("provider")
                .and_then(|x| x.as_str())
                .or_else(|| cfg.get("provider_plugin").and_then(|x| x.as_str()))
                .unwrap_or("provider-mock")
                .to_string();
            let base_url = params
                .get("base_url")
                .and_then(|x| x.as_str())
                .or_else(|| cfg.get("base_url").and_then(|x| x.as_str()))
                .unwrap_or("http://127.0.0.1:11434")
                .to_string();
            let model = params
                .get("model")
                .and_then(|x| x.as_str())
                .or_else(|| cfg.get("model").and_then(|x| x.as_str()))
                .unwrap_or("qwen2.5:7b")
                .to_string();
            let Ok(p) = kernel.plugins.get(&provider) else {
                return Ok(
                    json!({"provider": provider, "models": [], "error": "provider 插件未安装"}),
                );
            };
            match p.call("probe", json!({"base_url": base_url, "model": model})) {
                Ok(v) => {
                    let mut models: Vec<String> = Vec::new();
                    for m in v
                        .get("models")
                        .and_then(|x| x.as_array())
                        .into_iter()
                        .flatten()
                        .filter_map(|x| x.as_str())
                    {
                        if !models.iter().any(|x| x == m) {
                            models.push(m.to_string());
                        }
                    }
                    Ok(json!({"provider": provider, "models": models, "probe": v}))
                }
                Err(e) => Ok(json!({"provider": provider, "models": [], "error": e.to_string()})),
            }
        }
        "plugin.list" => {
            let reg = kernel.registry.list();
            let live = kernel.plugins.list();
            Ok(json!({
                "registry": reg,
                "live": live.iter().map(|(n, st, m)| json!({"name": n, "state": st, "manifest": m})).collect::<Vec<_>>(),
            }))
        }
        "plugin.enable" | "plugin.disable" => {
            let name = params
                .get("name")
                .and_then(|x| x.as_str())
                .ok_or_else(|| invalid("name required"))?;
            let on = method == "plugin.enable";
            kernel
                .registry
                .enable(name, on)
                .ok_or_else(|| (rpc::ERR_NO_SESSION, "插件未登记".to_string()))?;
            Ok(json!({"ok": true, "enabled": on}))
        }
        "plugin.install" => {
            let path = params
                .get("path")
                .and_then(|x| x.as_str())
                .ok_or_else(|| invalid("path required"))?;
            let e = kernel
                .registry
                .install(Path::new(path))
                .map_err(|e| (rpc::ERR_INTERNAL, e.to_string()))?;
            Ok(json!({"installed": e}))
        }
        _ => Err((rpc::ERR_METHOD_NOT_FOUND, format!("未知方法 {method}"))),
    }
}

/// 供 bench / doctor 直接嵌入使用时构造一条内存内 UDS 服务器。
pub fn spawn_server(
    kernel: Arc<Kernel>,
    socket: PathBuf,
    codec: Codec,
) -> std::io::Result<(Arc<Server>, thread::JoinHandle<()>)> {
    let srv = Server::new(kernel, socket, codec);
    let s2 = srv.clone();
    let h = thread::spawn(move || {
        let _ = s2.run();
    });
    Ok((srv, h))
}

/// 等待 socket 可连接（冷启动测量用）。
pub fn wait_ready(socket: &Path, timeout: Duration) -> std::io::Result<()> {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if crate::ipc::connect(socket).is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(1));
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "socket not ready",
    ))
}
