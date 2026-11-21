//! 前端契约服务（设计文档 §7.2）。
//!
//! 传输分层是硬约束：**内核只提供 UDS / 命名管道**；HTTP/WS 归 GUI 包的
//! `featheragent-gui` 桥接器，内核不实现任何 HTTP 服务（否则 8 MB 预算破功）。
//!
//! 最小方法集见文档 §7.2；帧为长度前缀 msgpack（v1 可退 JSON）。

pub mod client;

use std::collections::HashMap;
use std::io::BufReader;
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
        // 触发一次连接，唤醒 accept 循环
        let _ = std::os::unix::net::UnixStream::connect(&self.socket);
    }

    /// 阻塞运行；返回即表示停止。
    pub fn run(self: Arc<Self>) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if self.socket.exists() {
                let _ = std::fs::remove_file(&self.socket);
            }
            if let Some(dir) = self.socket.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let listener = std::os::unix::net::UnixListener::bind(&self.socket)?;
            let _ = std::fs::set_permissions(&self.socket, std::fs::Permissions::from_mode(0o600));
            listener.set_nonblocking(true)?;
            loop {
                if self.stop.load(Ordering::SeqCst) {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
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
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(20));
                    }
                    Err(e) => return Err(e),
                }
            }
            let _ = std::fs::remove_file(&self.socket);
            Ok(())
        }
        #[cfg(not(unix))]
        {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Windows 命名管道后端尚未实现（见 docs/验证报告.md 未测项）",
            ))
        }
    }
}

fn handle_conn(kernel: Arc<Kernel>, stream: std::os::unix::net::UnixStream, codec: Codec) -> std::io::Result<()> {
    let conn_id = kernel.next_conn_id();
    let (tx, rx): (Sender<Value>, Receiver<Value>) = mpsc::channel();
    kernel.broadcasts.write().push((conn_id, tx.clone()));

    // writer thread
    let mut wstream = stream.try_clone()?;
    let writer = thread::spawn(move || {
        while let Ok(v) = rx.recv() {
            if write_frame(&mut wstream, &v, Codec::MsgPack).is_err() {
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
                        let Some(req) = Request::parse(&v) else { continue };
                        if req.is_notification() {
                            continue;
                        }
                        let id = req.id.unwrap();
                        let resp = match dispatch(&kernel, conn_id, &kinds, &req.method, &req.params) {
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
    method: &str,
    params: &Value,
) -> RpcResult {
    match method {
        "initialize" => Ok(json!({
            "version": crate::VERSION,
            "contract": crate::proto::CONTRACT_API,
            "transport": "uds",
            "codec": "msgpack",
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
            let id = params.get("session").and_then(|x| x.as_str()).ok_or_else(|| invalid("session required"))?;
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
            kernel.emit(id, EventKind::SessionResumed, json!({"conn": conn_id, "role": role}));
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
            let id = params.get("session").and_then(|x| x.as_str()).ok_or_else(|| invalid("session required"))?;
            let as_id = params.get("as").and_then(|x| x.as_str()).unwrap_or("fork");
            let n = kernel
                .store
                .fork(id, as_id)
                .map_err(|e| (rpc::ERR_INTERNAL, e.to_string()))?;
            kernel.session(as_id);
            Ok(json!({"session": as_id, "copied": n}))
        }
        "chat.send" => {
            let id = params.get("session").and_then(|x| x.as_str()).ok_or_else(|| invalid("session required"))?;
            let text = params.get("text").and_then(|x| x.as_str()).ok_or_else(|| invalid("text required"))?;
            let s = kernel.session(id);
            let holder = *s.holder.read();
            match holder {
                Some(h) if h != conn_id => {
                    return Err((rpc::ERR_OBSERVER_READONLY, "当前连接是观察视图（单写者模型）".into()))
                }
                None => {
                    *s.holder.write() = Some(conn_id);
                }
                _ => {}
            }
            if *s.state.read() != SessionState::Idle {
                return Err((rpc::ERR_OCCUPIED, "会话正忙".into()));
            }
            let k = kernel.clone();
            let ss = s.clone();
            let t = text.to_string();
            thread::spawn(move || {
                if let Err(e) = k.run_turn(&ss, t) {
                    *ss.state.write() = SessionState::Failed;
                    *ss.error.write() = Some(e.clone());
                    k.emit(&ss.id, EventKind::ChatFinished, json!({"status": "failed", "error": e}));
                }
            });
            Ok(json!({"session": id, "accepted": true, "turn": s.turn.load(Ordering::Relaxed)}))
        }
        "chat.cancel" => {
            let id = params.get("session").and_then(|x| x.as_str()).ok_or_else(|| invalid("session required"))?;
            let s = kernel.session(id);
            s.cancel.store(true, Ordering::SeqCst);
            let active = s.active_req.lock().clone();
            if let Some((plugin, req)) = active {
                if let Ok(p) = kernel.plugins.get(&plugin) {
                    p.cancel(req);
                }
            }
            kernel.emit(id, EventKind::ToolCancelled, json!({"reason": "chat.cancel"}));
            Ok(json!({"ok": true}))
        }
        "event.subscribe" => {
            let ks = params.get("kinds").and_then(|x| x.as_array()).cloned().unwrap_or_default();
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
            let token = params.get("token").and_then(|x| x.as_str()).ok_or_else(|| invalid("token required"))?;
            let allow = method == "tool.confirm" && params.get("allow").and_then(|x| x.as_bool()).unwrap_or(true);
            Ok(json!({"ok": kernel.resolve_confirm(token, allow)}))
        }
        "ctx.inspect" => {
            let id = params.get("session").and_then(|x| x.as_str()).ok_or_else(|| invalid("session required"))?;
            let s = kernel.session(id);
            Ok(json!({
                "session": id,
                "state": s.state.read().as_str(),
                "snapshot": s.snapshot.read().clone(),
                "turn": s.turn.load(Ordering::Relaxed),
            }))
        }
        "config.get" => {
            let cfg = kernel.config_json();
            match params.get("path").and_then(|x| x.as_str()) {
                Some(p) => Ok(json!({"value": cfg.pointer(p).cloned().unwrap_or(Value::Null), "config": cfg})),
                None => Ok(cfg),
            }
        }
        "config.set" => {
            let patch = params.get("patch").cloned().or_else(|| params.get("value").cloned()).unwrap_or(Value::Null);
            let cfg = kernel
                .set_config(&patch)
                .map_err(|e| (rpc::ERR_INVALID_PARAMS, e))?;
            Ok(json!({"config": cfg}))
        }
        "storage.stats" => Ok(kernel.storage_stats()),
        "storage.gc" => {
            let dry = params.get("dry_run").and_then(|x| x.as_bool()).unwrap_or(true);
            Ok(kernel.storage_gc(dry))
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
            let name = params.get("name").and_then(|x| x.as_str()).ok_or_else(|| invalid("name required"))?;
            let on = method == "plugin.enable";
            kernel
                .registry
                .enable(name, on)
                .ok_or_else(|| (rpc::ERR_NO_SESSION, "插件未登记".to_string()))?;
            Ok(json!({"ok": true, "enabled": on}))
        }
        "plugin.install" => {
            let path = params.get("path").and_then(|x| x.as_str()).ok_or_else(|| invalid("path required"))?;
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
pub fn spawn_server(kernel: Arc<Kernel>, socket: PathBuf, codec: Codec) -> std::io::Result<(Arc<Server>, thread::JoinHandle<()>)> {
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
        if std::os::unix::net::UnixStream::connect(socket).is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(1));
    }
    Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "socket not ready"))
}
