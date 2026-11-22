//! 前端契约客户端：TUI / WebUI 桥 / 测试共用的 UDS RPC 客户端。
//!
//! 单连接单写者（内核侧保证），客户端内部用响应 id 路由，通知走事件通道。

use std::collections::HashMap;
use std::io::{BufReader, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use parking_lot::Mutex;
use serde_json::Value;

use crate::proto::rpc;
use crate::proto::{read_frame, write_frame, Codec};

struct Inner {
    tx: Mutex<crate::ipc::Stream>,
    pending: Mutex<HashMap<u64, Sender<Value>>>,
    next: AtomicU64,
}

pub struct Client {
    inner: Arc<Inner>,
}

pub struct Connect {
    pub client: Client,
    pub events: Receiver<Value>,
}

impl Client {
    pub fn connect(path: &Path) -> std::io::Result<Connect> {
        let stream = crate::ipc::connect(path)?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let inner = Arc::new(Inner {
            tx: Mutex::new(stream.try_clone()?),
            pending: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
        });
        let (etx, erx) = mpsc::channel::<Value>();
        let weak = Arc::downgrade(&inner);
        thread::spawn(move || loop {
            match read_frame(&mut reader, Codec::MsgPack) {
                Ok(Some(v)) => {
                    let Some(s) = weak.upgrade() else { break };
                    if let Some(id) = v.get("id").and_then(|x| x.as_u64()) {
                        if let Some(tx) = s.pending.lock().remove(&id) {
                            let _ = tx.send(v);
                            continue;
                        }
                    }
                    if etx.send(v).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(_) => break,
            }
        });
        Ok(Connect {
            client: Client { inner },
            events: erx,
        })
    }

    pub fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        self.call_with_timeout(method, params, Duration::from_secs(30))
    }

    pub fn call_with_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        let id = self.inner.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.inner.pending.lock().insert(id, tx);
        let req = rpc::request(id, method, params);
        {
            let mut w = self.inner.tx.lock();
            write_frame(&mut *w, &req, Codec::MsgPack).map_err(|e| e.to_string())?;
        }
        match rx.recv_timeout(timeout) {
            Ok(v) => {
                if let Some(err) = v.get("error") {
                    Err(format!(
                        "rpc {}: {}",
                        err.get("code").and_then(|c| c.as_i64()).unwrap_or(0),
                        err.get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("unknown")
                    ))
                } else {
                    Ok(v.get("result").cloned().unwrap_or(Value::Null))
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                self.inner.pending.lock().remove(&id);
                Err("timeout".into())
            }
            Err(RecvTimeoutError::Disconnected) => Err("connection closed".into()),
        }
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        let v = rpc::notification(method, params);
        let mut w = self.inner.tx.lock();
        write_frame(&mut *w, &v, Codec::MsgPack).map_err(|e| e.to_string())
    }

    pub fn close(&self) {
        let w = self.inner.tx.lock();
        let _ = w.shutdown(std::net::Shutdown::Both);
    }
}

/// 简单等待：测试里轮询事件直到谓词满足。
pub fn wait_event(
    rx: &Receiver<Value>,
    pred: impl Fn(&Value) -> bool,
    timeout: Duration,
) -> Option<Value> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(v) => {
                if pred(&v) {
                    return Some(v);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return None,
        }
    }
    None
}

pub fn is_event(v: &Value, kind: &str) -> bool {
    v.get("method").and_then(|m| m.as_str()) == Some("event")
        && v.get("params")
            .and_then(|p| p.get("kind"))
            .and_then(|k| k.as_str())
            == Some(kind)
}

pub fn event_payload(v: &Value) -> Value {
    v.get("params")
        .and_then(|p| p.get("payload"))
        .cloned()
        .unwrap_or(Value::Null)
}

#[allow(dead_code)]
fn _assert_io_impl() {
    fn _f<T: Read + Write>() {}
}
