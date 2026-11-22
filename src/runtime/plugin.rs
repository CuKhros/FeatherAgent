//! 进程外插件监管：握手、调用、流式通知、崩溃重试、取消传播。
//!
//! 传输：stdio + 长度前缀 msgpack 帧（`Codec::MsgPack`）；`--json` 时退回 JSON-RPC 兼容模式。

use std::collections::HashMap;
use std::io::{BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Weak};
use std::thread;
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::governance::sandbox::{self, SandboxCfg};
use crate::proto::types::{PluginManifest, CONTRACT_API};
use crate::proto::{read_frame, write_frame, Codec};

#[derive(Debug, Clone)]
pub struct PluginSpec {
    pub name: String,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub codec: Codec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginState {
    Starting,
    Ready,
    Degraded,
    Stopped,
    Crashed,
}

#[derive(Debug, Clone)]
pub enum PluginError {
    Io(String),
    Crashed,
    Timeout,
    Rpc { code: i64, message: String },
    ApiIncompatible(String),
    NotReady,
    NotInstalled(String),
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginError::Io(s) => write!(f, "io: {s}"),
            PluginError::Crashed => write!(f, "plugin crashed"),
            PluginError::Timeout => write!(f, "plugin call timeout"),
            PluginError::Rpc { code, message } => write!(f, "rpc {code}: {message}"),
            PluginError::ApiIncompatible(s) => write!(f, "api incompatible: {s}"),
            PluginError::NotReady => write!(f, "plugin not ready"),
            PluginError::NotInstalled(s) => write!(f, "plugin not installed: {s}"),
        }
    }
}

pub struct PluginProc {
    pub name: String,
    pub spec: PluginSpec,
    manifest: RwLock<Option<PluginManifest>>,
    state: RwLock<PluginState>,
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    pending: Mutex<HashMap<u64, Sender<Value>>>,
    streams: Mutex<HashMap<u64, Sender<Value>>>,
    next_id: AtomicU64,
    failures: AtomicU32,
    stderr_tail: Mutex<String>,
    call_timeout: Duration,
}

const MAX_FAILURES_BEFORE_DEGRADE: u32 = 3;

impl PluginProc {
    /// 拉起进程并完成 `initialize` 握手。
    pub fn spawn(
        spec: PluginSpec,
        sandbox_cfg: &SandboxCfg,
        call_timeout: Duration,
        env: &[(String, String)],
    ) -> Result<Arc<Self>, PluginError> {
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        sandbox::apply(&mut cmd, sandbox_cfg);
        for (k, v) in env {
            cmd.env(k, v);
        }
        // 本地 provider 不应被宿主机的 HTTP(S)_PROXY 截走；保留已有 NO_PROXY 并补齐回环地址。
        let existing = std::env::var("NO_PROXY")
            .or_else(|_| std::env::var("no_proxy"))
            .unwrap_or_default();
        let mut no_proxy = existing.clone();
        for host in ["127.0.0.1", "localhost", "::1"] {
            if !no_proxy.split(',').any(|x| x.trim() == host) {
                if !no_proxy.is_empty() {
                    no_proxy.push(',');
                }
                no_proxy.push_str(host);
            }
        }
        if no_proxy != existing {
            cmd.env("NO_PROXY", &no_proxy).env("no_proxy", &no_proxy);
        }
        let mut child = cmd.spawn().map_err(|e| PluginError::Io(e.to_string()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or(PluginError::Io("no stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or(PluginError::Io("no stdout".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or(PluginError::Io("no stderr".into()))?;

        let proc = Arc::new(Self {
            name: spec.name.clone(),
            spec,
            manifest: RwLock::new(None),
            state: RwLock::new(PluginState::Starting),
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            pending: Mutex::new(HashMap::new()),
            streams: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            failures: AtomicU32::new(0),
            stderr_tail: Mutex::new(String::new()),
            call_timeout,
        });

        // stdout reader
        let weak: Weak<PluginProc> = Arc::downgrade(&proc);
        thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            loop {
                match read_frame(&mut r, Codec::MsgPack) {
                    Ok(Some(v)) => {
                        if let Some(p) = weak.upgrade() {
                            p.dispatch(v);
                        } else {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
            if let Some(p) = weak.upgrade() {
                p.mark_crashed();
            }
        });

        // stderr drain（只保留尾部，避免占内存）
        let weak2: Weak<PluginProc> = Arc::downgrade(&proc);
        thread::spawn(move || {
            let mut r = BufReader::new(stderr);
            let mut buf = Vec::new();
            let _ = r.read_to_end(&mut buf);
            if let Some(p) = weak2.upgrade() {
                let s = String::from_utf8_lossy(&buf);
                let mut tail = p.stderr_tail.lock();
                let take = s.len().saturating_sub(8192);
                *tail = s[take..].to_string();
            }
        });

        // 握手
        let init = proc.call(
            "initialize",
            json!({
                "contract": "plugin",
                "api": CONTRACT_API,
                "client": "featheragent",
                "codec": "msgpack",
            }),
        )?;
        let manifest: PluginManifest = serde_json::from_value(init)
            .map_err(|e| PluginError::ApiIncompatible(format!("bad manifest: {e}")))?;
        check_api(&manifest.api)?;
        *proc.manifest.write() = Some(manifest);
        *proc.state.write() = PluginState::Ready;
        Ok(proc)
    }

    pub fn manifest(&self) -> Option<PluginManifest> {
        self.manifest.read().clone()
    }

    pub fn state(&self) -> PluginState {
        *self.state.read()
    }

    pub fn failures(&self) -> u32 {
        self.failures.load(Ordering::Relaxed)
    }

    pub fn stderr_tail(&self) -> String {
        self.stderr_tail.lock().clone()
    }

    fn dispatch(self: &Arc<Self>, v: Value) {
        if let Some(id) = v.get("id").and_then(|x| x.as_u64()) {
            // 响应
            if let Some(tx) = self.pending.lock().remove(&id) {
                let _ = tx.send(v);
                return;
            }
            // 流式响应也可能带 id
            if let Some(tx) = self.streams.lock().get(&id) {
                let _ = tx.send(v);
                return;
            }
        }
        // 通知
        let req = v
            .get("params")
            .and_then(|p| p.get("req"))
            .and_then(|x| x.as_u64());
        if let Some(req) = req {
            if let Some(tx) = self.streams.lock().get(&req) {
                let _ = tx.send(v);
                return;
            }
        }
    }

    fn mark_crashed(self: &Arc<Self>) {
        *self.state.write() = PluginState::Crashed;
        // 丢弃所有等待者 → 调用方收到 Crashed
        self.pending.lock().clear();
        self.streams.lock().clear();
    }

    fn write_msg(&self, v: &Value) -> Result<(), PluginError> {
        let mut w = self.stdin.lock();
        write_frame(&mut *w, v, self.spec.codec).map_err(|e| PluginError::Io(e.to_string()))
    }

    pub fn call(&self, method: &str, params: Value) -> Result<Value, PluginError> {
        self.call_with_timeout(method, params, self.call_timeout)
    }

    pub fn call_with_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, PluginError> {
        if matches!(self.state(), PluginState::Crashed | PluginState::Stopped) {
            return Err(PluginError::Crashed);
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.pending.lock().insert(id, tx);
        let req = crate::proto::rpc::request(id, method, params);
        if let Err(e) = self.write_msg(&req) {
            self.pending.lock().remove(&id);
            return Err(e);
        }
        match rx.recv_timeout(timeout) {
            Ok(v) => {
                if let Some(err) = v.get("error") {
                    let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(-32000);
                    let message = err
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    Err(PluginError::Rpc { code, message })
                } else {
                    Ok(v.get("result").cloned().unwrap_or(Value::Null))
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                self.pending.lock().remove(&id);
                self.failures.fetch_add(1, Ordering::Relaxed);
                Err(PluginError::Timeout)
            }
            Err(RecvTimeoutError::Disconnected) => {
                self.pending.lock().remove(&id);
                Err(PluginError::Crashed)
            }
        }
    }

    /// 发起流式请求：返回 `(req_id, 事件接收器)`。
    /// 插件以 `plugin.event` 通知回传 `{req, kind: delta|done|error, data}`。
    pub fn start_stream(
        &self,
        method: &str,
        mut params: Value,
    ) -> Result<(u64, Receiver<Value>), PluginError> {
        if matches!(self.state(), PluginState::Crashed | PluginState::Stopped) {
            return Err(PluginError::Crashed);
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.streams.lock().insert(id, tx);
        if let Some(obj) = params.as_object_mut() {
            obj.insert("req".into(), json!(id));
        }
        let req = crate::proto::rpc::request(id, method, params);
        if let Err(e) = self.write_msg(&req) {
            self.streams.lock().remove(&id);
            return Err(e);
        }
        Ok((id, rx))
    }

    pub fn finish_stream(&self, req: u64) {
        self.streams.lock().remove(&req);
    }

    /// 取消传播：先发 `$/cancel`，3 秒后仍在跑则由调用方 `hard_kill`（§13）。
    pub fn cancel(&self, req: u64) {
        let _ = self.write_msg(&json!({
            "jsonrpc": "2.0",
            "method": "$/cancel",
            "params": {"req": req},
        }));
    }

    /// SIGTERM → 等待宽限期 → SIGKILL。
    pub fn terminate(&self, grace: Duration) {
        #[cfg(unix)]
        {
            let pid = {
                let c = self.child.lock();
                c.id() as i32
            };
            unsafe { libc::kill(pid, libc::SIGTERM) };
            let deadline = Instant::now() + grace;
            while Instant::now() < deadline {
                {
                    let mut c = self.child.lock();
                    if let Ok(Some(_)) = c.try_wait() {
                        *self.state.write() = PluginState::Stopped;
                        return;
                    }
                }
                thread::sleep(Duration::from_millis(50));
            }
            let mut c = self.child.lock();
            let _ = c.kill();
            let _ = c.wait();
        }
        #[cfg(not(unix))]
        {
            let _ = grace;
            let mut c = self.child.lock();
            let _ = c.kill();
            let _ = c.wait();
        }
        *self.state.write() = PluginState::Stopped;
    }

    /// 记录一次失败；返回是否应熔断（连续 3 次）。
    pub fn record_failure(&self) -> bool {
        let n = self.failures.fetch_add(1, Ordering::Relaxed) + 1;
        if n >= MAX_FAILURES_BEFORE_DEGRADE {
            *self.state.write() = PluginState::Degraded;
            true
        } else {
            false
        }
    }

    pub fn reset_failures(&self) {
        self.failures.store(0, Ordering::Relaxed);
        if self.state() == PluginState::Degraded {
            *self.state.write() = PluginState::Ready;
        }
    }
}

impl Drop for PluginProc {
    fn drop(&mut self) {
        let mut c = self.child.lock();
        let _ = c.kill();
        let _ = c.wait();
    }
}

/// 解析 `>=1.0 <2.0` 形式的区间，校验 `CONTRACT_API`。
pub fn check_api(range: &str) -> Result<(), PluginError> {
    let mut lower: Option<f64> = None;
    let mut upper: Option<f64> = None;
    for part in range.split_whitespace() {
        let (op, ver) = if let Some(v) = part.strip_prefix(">=") {
            (">=", v)
        } else if let Some(v) = part.strip_prefix('<') {
            ("<", v)
        } else if let Some(v) = part.strip_prefix('>') {
            (">", v)
        } else if let Some(v) = part.strip_prefix("<=") {
            ("<=", v)
        } else {
            ("=", part)
        };
        let n: f64 = ver
            .parse()
            .map_err(|_| PluginError::ApiIncompatible(format!("bad version {ver}")))?;
        match op {
            ">=" => lower = Some(lower.map_or(n, |x: f64| x.max(n))),
            "<" => upper = Some(upper.map_or(n, |x: f64| x.min(n))),
            ">" => lower = Some(lower.map_or(n, |x: f64| x.max(n))),
            "<=" => upper = Some(upper.map_or(n, |x: f64| x.min(n))),
            _ => {
                lower = Some(n);
                upper = Some(n + 0.0001);
            }
        }
    }
    let me: f64 = CONTRACT_API.parse().unwrap_or(1.0);
    if let Some(l) = lower {
        if me < l {
            return Err(PluginError::ApiIncompatible(format!(
                "需要 {range}，内核 {CONTRACT_API}"
            )));
        }
    }
    if let Some(u) = upper {
        if me >= u {
            return Err(PluginError::ApiIncompatible(format!(
                "需要 {range}，内核 {CONTRACT_API}"
            )));
        }
    }
    Ok(())
}

/// 插件集合：命名 → 进程。负责拉起、重启、熔断。
pub struct PluginHost {
    procs: RwLock<HashMap<String, Arc<PluginProc>>>,
    specs: RwLock<HashMap<String, PluginSpec>>,
    sandbox: SandboxCfg,
    call_timeout: Duration,
    restarts: AtomicU32,
    env: Vec<(String, String)>,
}

impl PluginHost {
    pub fn new(sandbox: SandboxCfg, call_timeout: Duration, env: Vec<(String, String)>) -> Self {
        Self {
            procs: RwLock::new(HashMap::new()),
            specs: RwLock::new(HashMap::new()),
            sandbox,
            call_timeout,
            restarts: AtomicU32::new(0),
            env,
        }
    }

    fn spawn_spec(&self, spec: PluginSpec) -> Result<Arc<PluginProc>, PluginError> {
        PluginProc::spawn(spec, &self.sandbox, self.call_timeout, &self.env)
    }

    pub fn register_spec(&self, spec: PluginSpec) {
        self.specs.write().insert(spec.name.clone(), spec);
    }

    pub fn specs(&self) -> Vec<PluginSpec> {
        self.specs.read().values().cloned().collect()
    }

    pub fn get(&self, name: &str) -> Result<Arc<PluginProc>, PluginError> {
        if let Some(p) = self.procs.read().get(name) {
            return Ok(p.clone());
        }
        let spec = self
            .specs
            .read()
            .get(name)
            .cloned()
            .ok_or_else(|| PluginError::NotInstalled(name.to_string()))?;
        let p = self.spawn_spec(spec)?;
        self.procs.write().insert(name.to_string(), p.clone());
        Ok(p)
    }

    /// 崩溃后重启（重试 1 次用），并清空失败计数。
    pub fn restart(&self, name: &str) -> Result<Arc<PluginProc>, PluginError> {
        {
            let mut m = self.procs.write();
            if let Some(old) = m.remove(name) {
                old.terminate(Duration::from_millis(300));
            }
        }
        self.restarts.fetch_add(1, Ordering::Relaxed);
        let spec = self
            .specs
            .read()
            .get(name)
            .cloned()
            .ok_or_else(|| PluginError::NotInstalled(name.to_string()))?;
        let p = self.spawn_spec(spec)?;
        p.reset_failures();
        self.procs.write().insert(name.to_string(), p.clone());
        Ok(p)
    }

    pub fn list(&self) -> Vec<(String, PluginState, Option<PluginManifest>)> {
        self.specs
            .read()
            .keys()
            .map(|n| {
                let p = self.procs.read().get(n).cloned();
                match p {
                    Some(p) => (n.clone(), p.state(), p.manifest()),
                    None => (n.clone(), PluginState::Stopped, None),
                }
            })
            .collect()
    }

    pub fn stop_all(&self) {
        for (_, p) in self.procs.read().iter() {
            p.terminate(Duration::from_millis(300));
        }
        self.procs.write().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_range_checks() {
        assert!(check_api(">=1.0 <2.0").is_ok());
        assert!(check_api(">=2.0 <3.0").is_err());
        assert!(check_api(">=0.5 <1.0").is_err());
        assert!(check_api(">=1.0").is_ok());
    }
}
