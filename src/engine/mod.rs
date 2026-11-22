//! 编排核心（设计文档 §2.1 A / §13 数据流）。
//!
//! 内核**不认识**任何具体能力：会话循环只看事件与契约，
//! provider / tool / storage 全部经 `PluginHost` 的进程外契约调用。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Weak};
use std::thread;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::bus::Bus;
use crate::context::{
    ContextEngine, CtxConfig, ModelProfile, RecallItem, Skill,
};
use crate::governance::paths::Paths;
use crate::governance::permission::{Decision, Gate};
use crate::governance::sandbox::SandboxCfg;
use crate::governance::volume::{self, Purpose, Verdict};
use crate::log::LogStore;
use crate::proto::types::{Event, EventKind};
use crate::runtime::registry::Registry;
use crate::runtime::{PluginError, PluginHost, PluginState};
use crate::util::now_ms;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub provider_plugin: String,
    pub tool_plugin: String,
    pub storage_plugin: String,
    pub model: String,
    pub base_url: String,
    pub max_turns: u32,
    /// `always | interval | never`
    pub sync: String,
    pub strict_temp: bool,
    pub segment_size: u64,
    pub context_window: usize,
    pub quota_bytes: u64,
    pub auto_allow_safe: bool,
    pub allowed_sync_names: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider_plugin: "provider-mock".into(),
            tool_plugin: "tool".into(),
            storage_plugin: "storage".into(),
            model: "qwen2.5:7b".into(),
            base_url: "http://127.0.0.1:11434".into(),
            max_turns: 6,
            sync: "interval".into(),
            strict_temp: true,
            segment_size: 4 * 1024 * 1024,
            context_window: 8192,
            quota_bytes: 4 * 1024 * 1024 * 1024,
            auto_allow_safe: true,
            allowed_sync_names: Vec::new(),
        }
    }
}

impl Config {
    pub fn load(path: &PathBuf) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &PathBuf) -> std::io::Result<()> {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).unwrap())?;
        crate::util::replace_file(&tmp, path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Idle,
    Assembling,
    Generating,
    ToolExecuting,
    Cancelled,
    Failed,
}

impl SessionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionState::Idle => "idle",
            SessionState::Assembling => "assembling",
            SessionState::Generating => "generating",
            SessionState::ToolExecuting => "tool_executing",
            SessionState::Cancelled => "cancelled",
            SessionState::Failed => "failed",
        }
    }
}

pub struct Session {
    pub id: String,
    pub state: RwLock<SessionState>,
    pub holder: RwLock<Option<u64>>,
    pub cancel: AtomicBool,
    pub turn: AtomicU32,
    pub snapshot: RwLock<Value>,
    pub locked_tools: Vec<Value>,
    pub locked_skills: Vec<Skill>,
    pub active_req: Mutex<Option<(String, u64)>>,
    pub error: RwLock<Option<String>>,
    pub created_ms: u128,
}

impl Session {
    pub fn new(id: &str, tools: Vec<Value>, skills: Vec<Skill>) -> Self {
        Self {
            id: id.to_string(),
            state: RwLock::new(SessionState::Idle),
            holder: RwLock::new(None),
            cancel: AtomicBool::new(false),
            turn: AtomicU32::new(0),
            snapshot: RwLock::new(Value::Null),
            locked_tools: tools,
            locked_skills: skills,
            active_req: Mutex::new(None),
            error: RwLock::new(None),
            created_ms: now_ms(),
        }
    }

    pub fn info(&self) -> Value {
        let holder = self.holder.read().clone();
        let error = self.error.read().clone();
        json!({
            "id": self.id,
            "state": self.state.read().as_str(),
            "holder": holder,
            "turn": self.turn.load(Ordering::Relaxed),
            "created_ms": self.created_ms,
            "error": error,
        })
    }
}

pub struct Kernel {
    pub paths: Paths,
    pub cfg: RwLock<Config>,
    pub cfg_path: PathBuf,
    pub store: Arc<LogStore>,
    pub bus: Arc<Bus>,
    pub plugins: Arc<PluginHost>,
    pub registry: Arc<Registry>,
    pub gate: Arc<Gate>,
    pub ctx: ContextEngine,
    pub sessions: RwLock<HashMap<String, Arc<Session>>>,
    pub broadcasts: RwLock<Vec<(u64, Sender<Value>)>>,
    pub confirms: Mutex<HashMap<String, Sender<bool>>>,
    next_conn: AtomicU64,
    pub started: Instant,
    index_tx: Mutex<Option<Sender<(String, Arc<Event>)>>>,
    pub warnings: RwLock<Vec<String>>,
}

struct ModelReply {
    text: String,
    finish: String,
    tool_calls: Vec<crate::context::parse::ToolCall>,
}

impl Kernel {
    pub fn new(paths: Paths, cfg: Config, force: bool) -> Result<Arc<Self>, String> {
        paths.create_all().map_err(|e| format!("创建目录失败: {e}"))?;

        // 卷类型守卫（§3.5）：数据目录在网络盘/同步盘拒绝启动
        let mut warnings = Vec::new();
        for (p, purpose) in [
            (paths.data.clone(), Purpose::Data),
            (paths.workspace.clone(), Purpose::Workspace),
        ] {
            let kind = volume::classify(&p, &cfg.allowed_sync_names);
            let verdict = volume::judge(kind, purpose);
            let label = volume::label(kind);
            match verdict {
                Verdict::Reject if !force => {
                    return Err(format!(
                        "{} 位于 {label} 卷，按设计拒绝启动（可加 --force 覆盖）: {}",
                        match purpose {
                            Purpose::Data => "数据目录",
                            Purpose::Workspace => "工作空间",
                        },
                        p.display()
                    ));
                }
                Verdict::Reject => warnings.push(format!("{} 位于 {label} 卷（已 --force 覆盖）", p.display())),
                Verdict::Warn => warnings.push(format!("{} 位于 {label} 卷（仅告警）", p.display())),
                Verdict::Ok => {}
            }
        }

        let cfg_path = paths.data.join("config.json");
        let log_cfg = crate::log::LogConfig {
            segment_size: cfg.segment_size,
            sync: crate::log::SyncMode::parse(&cfg.sync).unwrap_or(crate::log::SyncMode::Interval),
            ..Default::default()
        };
        let store = Arc::new(
            LogStore::open(&paths.data, log_cfg).map_err(|e| format!("打开日志失败: {e}"))?,
        );
        let registry = Arc::new(
            Registry::load(&paths.data.join("plugins")).map_err(|e| format!("注册表失败: {e}"))?,
        );
        // 扫描内置插件目录
        let builtin = paths.install_root.join("plugins");
        if builtin.exists() {
            let _ = registry.scan_builtin(&builtin);
        }

        let sandbox = SandboxCfg {
            strict_temp: cfg.strict_temp,
            runtime_dir: paths.runtime.clone(),
        };
        let host = Arc::new(PluginHost::new(
            sandbox,
            Duration::from_millis(30_000),
            vec![
                ("FEATHER_DATA".into(), paths.data.to_string_lossy().into_owned()),
                ("FEATHER_WORKSPACE".into(), paths.workspace.to_string_lossy().into_owned()),
                ("FEATHER_RUNTIME".into(), paths.runtime.to_string_lossy().into_owned()),
            ],
        ));
        for spec in registry.specs() {
            host.register_spec(spec);
        }

        let mut gate = Gate::new(cfg.auto_allow_safe);
        gate.register("tool.shell", crate::proto::types::Risk::Dangerous);
        gate.register("tool.fs.write", crate::proto::types::Risk::Dangerous);
        gate.register("tool.fs.read", crate::proto::types::Risk::Safe);

        let bus = Arc::new(Bus::new(4096));
        let k = Arc::new(Self {
            paths,
            cfg: RwLock::new(cfg),
            cfg_path,
            store,
            bus,
            plugins: host,
            registry,
            gate: Arc::new(gate),
            ctx: ContextEngine::new(CtxConfig::default()),
            sessions: RwLock::new(HashMap::new()),
            broadcasts: RwLock::new(Vec::new()),
            confirms: Mutex::new(HashMap::new()),
            next_conn: AtomicU64::new(1),
            started: Instant::now(),
            index_tx: Mutex::new(None),
            warnings: RwLock::new(warnings),
        });

        // 索引器：异步 tail 日志 → storage 插件（不阻塞生者）
        let (tx, rx) = mpsc::channel();
        *k.index_tx.lock() = Some(tx);
        let weak: Weak<Kernel> = Arc::downgrade(&k);
        thread::spawn(move || indexer_loop(weak, rx));
        Ok(k)
    }

    pub fn next_conn_id(&self) -> u64 {
        self.next_conn.fetch_add(1, Ordering::Relaxed)
    }

    /// 追加事件 + 发布总线 + 交给索引器（§13 的双写）。
    pub fn emit(&self, session: &str, kind: EventKind, mut payload: Value) -> Arc<Event> {
        if let Some(obj) = payload.as_object_mut() {
            obj.entry("session").or_insert_with(|| json!(session));
        }
        match self.store.append(session, kind, payload) {
            Ok(ev) => {
                let ev = Arc::new(ev);
                self.bus.publish(ev.clone());
                if let Some(tx) = self.index_tx.lock().as_ref() {
                    let _ = tx.send((session.to_string(), ev.clone()));
                }
                ev
            }
            Err(e) => {
                let ev = Event::new(0, crate::proto::types::now_wall_ns(), kind, json!({"error": e.to_string()}));
                Arc::new(ev)
            }
        }
    }

    pub fn broadcast(&self, v: Value) {
        let mut list = self.broadcasts.write();
        list.retain(|(_, tx)| tx.send(v.clone()).is_ok());
    }

    pub fn session(&self, id: &str) -> Arc<Session> {
        if let Some(s) = self.sessions.read().get(id) {
            return s.clone();
        }
        let skills = self.load_skills();
        let s = Arc::new(Session::new(id, default_tools(), skills));
        self.sessions.write().insert(id.to_string(), s.clone());
        s
    }

    fn load_skills(&self) -> Vec<Skill> {
        let dir = self.paths.install_root.join("skills");
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let p = e.path().join("skill.yaml");
                if let Ok(text) = std::fs::read_to_string(&p) {
                    if let Some(s) = Skill::from_yaml(&text) {
                        out.push(s);
                    }
                }
            }
        }
        out
    }

    pub fn config_json(&self) -> Value {
        serde_json::to_value(self.cfg.read().clone()).unwrap_or(Value::Null)
    }

    pub fn set_config(&self, patch: &Value) -> Result<Value, String> {
        let mut cfg = self.cfg.read().clone();
        let cur = serde_json::to_value(&cfg).map_err(|e| e.to_string())?;
        let merged = merge_json(cur, patch.clone());
        cfg = serde_json::from_value(merged).map_err(|e| format!("配置非法: {e}"))?;
        cfg.save(&self.cfg_path).map_err(|e| e.to_string())?;
        *self.cfg.write() = cfg.clone();
        self.emit("_system", EventKind::ConfigChanged, json!({"patch": patch}));
        Ok(serde_json::to_value(cfg).unwrap_or(Value::Null))
    }

    /// L4 召回：优先向量（provider.embed → storage.vec.search），失败回落 FTS。
    pub fn recall(&self, text: &str, provider: &str) -> Vec<RecallItem> {
        let cfg = self.cfg.read().clone();
        let mut items = Vec::new();
        if let Ok(p) = self.plugins.get(provider) {
            if let Ok(res) = p.call("embed", json!({"text": text})) {
                if let Some(v) = res.get("vector").and_then(|v| v.as_array()) {
                    let vector: Vec<f32> = v.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect();
                    if !vector.is_empty() {
                        if let Ok(s) = self.plugins.get(&cfg.storage_plugin) {
                            if let Ok(r) = s.call(
                                "vec.search",
                                json!({"vector": vector, "k": 5, "model": res.get("model")}),
                            ) {
                                if let Some(arr) = r.get("items").and_then(|x| x.as_array()) {
                                    for it in arr {
                                        items.push(RecallItem {
                                            id: it.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                                            text: it.get("text").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                                            score: it.get("score").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
                                            source: "vector".into(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if items.is_empty() {
            if let Ok(s) = self.plugins.get(&cfg.storage_plugin) {
                if let Ok(r) = s.call("search.fts", json!({"q": text, "limit": 5})) {
                    if let Some(arr) = r.get("items").and_then(|x| x.as_array()) {
                        for it in arr {
                            items.push(RecallItem {
                                id: it.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                                text: it.get("text").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                                score: it.get("score").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
                                source: "fts".into(),
                            });
                        }
                    }
                }
            }
        }
        self.emit(
            "_system",
            EventKind::MemoryRecalled,
            json!({"count": items.len(), "query_chars": text.chars().count()}),
        );
        items
    }

    /// 存储统计：转发给 storage 插件；插件缺失时给出内核侧旁路统计。
    pub fn storage_stats(&self) -> Value {
        let cfg = self.cfg.read().clone();
        if let Ok(s) = self.plugins.get(&cfg.storage_plugin) {
            if let Ok(v) = s.call("stats", json!({})) {
                return v;
            }
        }
        let sessions = self.store.list_sessions().unwrap_or_default();
        let total: u64 = sessions.iter().map(|(_, n, _)| *n).sum();
        json!({"available": false, "sessions": sessions.len(), "records": total})
    }

    pub fn storage_gc(&self, dry_run: bool) -> Value {
        let cfg = self.cfg.read().clone();
        match self.plugins.get(&cfg.storage_plugin) {
            Ok(s) => s.call("gc", json!({"dry_run": dry_run})).unwrap_or_else(|e| json!({"error": e.to_string()})),
            Err(e) => json!({"error": e.to_string()}),
        }
    }

    /// 一次完整的 Agent 回合（§13）。
    pub fn run_turn(self: &Arc<Self>, session: &Arc<Session>, text: String) -> Result<(), String> {
        let cfg = self.cfg.read().clone();
        session.cancel.store(false, Ordering::SeqCst);
        *session.state.write() = SessionState::Assembling;
        *session.error.write() = None;

        self.emit(
            &session.id,
            EventKind::UserMessage,
            json!({"text": text, "ts": crate::proto::types::now_wall_ns()}),
        );

        // V4：向量召回写入（embed 能力可用时）
        self.index_user_text(session, &text, &cfg);

        let mut last_status = "ok";
        for turn in 0..cfg.max_turns.max(1) {
            if session.cancel.load(Ordering::SeqCst) {
                last_status = "cancelled";
                break;
            }
            session.turn.store(turn, Ordering::Relaxed);

            // ---- 组装上下文 ----
            *session.state.write() = SessionState::Assembling;
            let events = self
                .store
                .read_session(&session.id)
                .map_err(|e| format!("读日志失败: {e}"))?;
            let profile = ModelProfile {
                name: cfg.model.clone(),
                context_window: cfg.context_window,
                max_output: 512,
                capabilities: vec!["chat".into(), "stream".into(), "tools".into()],
            };
            let recalls = self.recall(&text, &cfg.provider_plugin);
            let assembled = self
                .ctx
                .assemble(&events, &profile, &session.locked_tools, &session.locked_skills, &recalls)
                .map_err(|e| e.to_string())?;
            *session.snapshot.write() = json!({
                "stats": assembled.stats,
                "cards": assembled.summary_cards,
                "task_state": assembled.task_state,
                "turns": events.len(),
                "model": cfg.model,
            });
            self.emit(
                &session.id,
                EventKind::CtxSnapshot,
                json!({"stats": assembled.stats, "cards": assembled.summary_cards.len()}),
            );

            // ---- 模型 ----
            *session.state.write() = SessionState::Generating;
            let mut model_res = self.run_model(session, &cfg, &assembled.messages, None);
            if let Err(e) = &model_res {
                // 空响应通常是采样抖动：换一点温度重试一次，仍空则硬失败。
                if e.contains("空响应") && !session.cancel.load(Ordering::SeqCst) {
                    self.emit(&session.id, EventKind::ModelRetry, json!({"attempt": 1, "reason": "empty", "temperature": 0.7}));
                    model_res = self.run_model(session, &cfg, &assembled.messages, Some(0.7));
                }
            }
            if let Err(e) = &model_res {
                // §4.5：插件崩溃 → 重启后重试一次
                if e.contains("崩溃") {
                    let _ = self.plugins.restart(&cfg.provider_plugin);
                    model_res = self.run_model(session, &cfg, &assembled.messages, None);
                }
            }
            let reply = model_res?;
            if session.cancel.load(Ordering::SeqCst) {
                last_status = "cancelled";
                break;
            }
            self.emit(&session.id, EventKind::ModelMessage, json!({"text": reply.text, "finish": reply.finish}));

            // ---- 工具 ----
            // provider 结构化返回的 tool_calls 优先；文本解析只作为兼容回落。
            let calls = if reply.tool_calls.is_empty() {
                crate::context::parse::extract(&reply.text)
            } else {
                reply.tool_calls.clone()
            };
            if calls.is_empty() {
                break;
            }
            *session.state.write() = SessionState::ToolExecuting;
            let mut executed = 0usize;
            for call in calls {
                if session.cancel.load(Ordering::SeqCst) {
                    self.emit(
                        &session.id,
                        EventKind::ToolCancelled,
                        json!({"name": call.name, "reason": "user_cancel"}),
                    );
                    last_status = "cancelled";
                    break;
                }
                self.emit(
                    &session.id,
                    EventKind::ToolCall,
                    json!({"name": call.name, "arguments": call.arguments}),
                );
                match self.authorize(session, &call.name) {
                    Ok(true) => {}
                    Ok(false) => {
                        self.emit(
                            &session.id,
                            EventKind::PermissionDenied,
                            json!({"tool": call.name, "reason": "user_denied"}),
                        );
                        self.emit(
                            &session.id,
                            EventKind::ToolResult,
                            json!({
                                "name": call.name,
                                "denied": true,
                                "result": {
                                    "error": "permission_denied",
                                    "reason": "user_denied",
                                    "tool": call.name,
                                },
                            }),
                        );
                        executed += 1;
                        continue;
                    }
                    Err(e) => {
                        self.emit(&session.id, EventKind::PermissionDenied, json!({"tool": call.name, "reason": e}));
                        self.emit(
                            &session.id,
                            EventKind::ToolResult,
                            json!({
                                "name": call.name,
                                "denied": true,
                                "result": {
                                    "error": "permission_denied",
                                    "reason": e,
                                    "tool": call.name,
                                },
                            }),
                        );
                        executed += 1;
                        continue;
                    }
                }
                match self.execute_tool(session, &cfg, &call.name, &call.arguments) {
                    Ok(v) => {
                        self.emit(&session.id, EventKind::ToolResult, json!({"name": call.name, "result": v}));
                        executed += 1;
                    }
                    Err(e) => {
                        self.emit(&session.id, EventKind::ToolFailed, json!({"name": call.name, "error": e}));
                        // 崩溃不中止会话：标记后继续推理（§4.5）
                        executed += 1;
                    }
                }
            }
            if session.cancel.load(Ordering::SeqCst) {
                break;
            }
            if executed == 0 {
                break;
            }
        }

        *session.state.write() = match last_status {
            "cancelled" => SessionState::Cancelled,
            _ => SessionState::Idle,
        };
        self.emit(&session.id, EventKind::ChatFinished, json!({"status": last_status}));
        Ok(())
    }

    fn index_user_text(&self, session: &Arc<Session>, text: &str, cfg: &Config) {
        let Ok(p) = self.plugins.get(&cfg.provider_plugin) else { return };
        let Ok(res) = p.call("embed", json!({"text": text})) else { return };
        let Some(v) = res.get("vector").and_then(|x| x.as_array()) else { return };
        let vector: Vec<f32> = v.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect();
        if vector.is_empty() {
            return;
        }
        if let Ok(s) = self.plugins.get(&cfg.storage_plugin) {
            let _ = s.call(
                "vec.upsert",
                json!({
                    "id": format!("{}:{}", session.id, crate::proto::types::now_wall_ns()),
                    "session": session.id,
                    "text": text,
                    "vector": vector,
                    "model": res.get("model").cloned().unwrap_or(json!(cfg.model)),
                }),
            );
        }
    }

    fn run_model(
        self: &Arc<Self>,
        session: &Arc<Session>,
        cfg: &Config,
        messages: &[crate::context::Message],
        temperature: Option<f64>,
    ) -> Result<ModelReply, String> {
        let proc = self.plugins.get(&cfg.provider_plugin).map_err(|e| e.to_string())?;
        let mut params = json!({
            "model": cfg.model,
            "messages": messages,
            "tools": session.locked_tools,
            "stream": true,
            "base_url": cfg.base_url,
        });
        if let Some(t) = temperature {
            params["temperature"] = json!(t);
        }
        let (req, rx) = proc.start_stream("chat", params).map_err(|e| e.to_string())?;
        *session.active_req.lock() = Some((cfg.provider_plugin.clone(), req));

        let mut acc = String::new();
        let mut finish = "stop".to_string();
        let mut tool_calls: Vec<crate::context::parse::ToolCall> = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(600);
        let mut first = true;
        loop {
            if session.cancel.load(Ordering::SeqCst) {
                proc.cancel(req);
                self.emit(&session.id, EventKind::ToolCancelled, json!({"name": "model.chat", "reason": "user_cancel"}));
                // 取消传播：SIGTERM 3s → SIGKILL 由 hard_kill 兜底
                break;
            }
            if Instant::now() > deadline {
                proc.finish_stream(req);
                *session.active_req.lock() = None;
                return Err("模型请求超时".into());
            }
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(v) => {
                    let p = v.get("params").cloned().unwrap_or(Value::Null);
                    let kind = p.get("kind").and_then(|k| k.as_str()).unwrap_or("");
                    match kind {
                        "delta" => {
                            let t = p.get("text").and_then(|t| t.as_str()).unwrap_or("");
                            if !t.is_empty() {
                                acc.push_str(t);
                                if first {
                                    self.emit(&session.id, EventKind::ModelDelta, json!({"text": t, "first": true}));
                                    first = false;
                                } else {
                                    self.emit(&session.id, EventKind::ModelDelta, json!({"text": t}));
                                }
                            }
                        }
                        "done" => {
                            if let Some(f) = p.get("finish_reason").and_then(|f| f.as_str()) {
                                finish = f.to_string();
                            }
                            if let Some(tc) = p.get("tool_calls") {
                                tool_calls = crate::context::parse::from_value(tc);
                            }
                            break;
                        }
                        "error" => {
                            let e = p.get("message").and_then(|m| m.as_str()).unwrap_or("provider error");
                            proc.finish_stream(req);
                            *session.active_req.lock() = None;
                            let _ = proc.record_failure();
                            return Err(e.to_string());
                        }
                        _ => {}
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    *session.active_req.lock() = None;
                    let degrade = proc.record_failure();
                    if degrade {
                        self.emit(&session.id, EventKind::PluginDegraded, json!({"plugin": cfg.provider_plugin}));
                    }
                    return Err("provider 崩溃".into());
                }
            }
        }
        proc.finish_stream(req);
        *session.active_req.lock() = None;
        if acc.is_empty() && tool_calls.is_empty() && finish == "stop" {
            finish = "empty".into();
        }
        Ok(ModelReply { text: acc, finish, tool_calls })
    }

    fn authorize(&self, session: &Arc<Session>, tool: &str) -> Result<bool, String> {
        match self.gate.check(tool, &session.id) {
            Decision::Allowed => Ok(true),
            Decision::ConfirmNeeded { token } => {
                let (tx, rx) = mpsc::channel();
                self.confirms.lock().insert(token.clone(), tx);
                self.broadcast(json!({
                    "jsonrpc": "2.0",
                    "method": "tool.confirm_request",
                    "params": {
                        "session": session.id,
                        "token": token,
                        "tool": tool,
                        "risk": "dangerous",
                    }
                }));
                match rx.recv_timeout(Duration::from_secs(120)) {
                    Ok(allowed) => Ok(allowed),
                    Err(_) => Err("确认超时".into()),
                }
            }
            Decision::Denied => Ok(false),
        }
    }

    pub fn resolve_confirm(&self, token: &str, allow: bool) -> bool {
        if let Some(tx) = self.confirms.lock().remove(token) {
            let ok = self.gate.resolve(token, allow);
            let _ = tx.send(allow);
            ok || allow
        } else {
            false
        }
    }

    fn execute_tool(
        &self,
        session: &Arc<Session>,
        cfg: &Config,
        name: &str,
        args: &Value,
    ) -> Result<Value, String> {
        let plugin_name = if name.starts_with("tool.") {
            cfg.tool_plugin.clone()
        } else if name.starts_with("storage.") {
            cfg.storage_plugin.clone()
        } else {
            cfg.tool_plugin.clone()
        };
        let mut last_err = String::new();
        for attempt in 0..2 {
            match self.plugins.get(&plugin_name) {
                Ok(proc) => match proc.call(name, json!({"arguments": args, "session": session.id, "workspace": session_workspace(self, &session.id) })) {
                    Ok(v) => {
                        proc.reset_failures();
                        return Ok(v);
                    }
                    Err(PluginError::Crashed) | Err(PluginError::Io(_)) if attempt == 0 => {
                        last_err = "插件崩溃，重启后重试一次".into();
                        let _ = self.plugins.restart(&plugin_name);
                        continue;
                    }
                    Err(e) => {
                        last_err = e.to_string();
                        let degrade = self.plugins.get(&plugin_name).map(|p| p.record_failure()).unwrap_or(false);
                        if degrade {
                            self.emit(&session.id, EventKind::PluginDegraded, json!({"plugin": plugin_name}));
                        }
                        break;
                    }
                },
                Err(e) => {
                    last_err = e.to_string();
                    break;
                }
            }
        }
        Err(last_err)
    }

    pub fn stop(&self) {
        self.plugins.stop_all();
    }
}

fn session_workspace(kernel: &Kernel, session: &str) -> String {
    kernel.paths.workspace.join(crate::log::safe_session_id(session)).to_string_lossy().into_owned()
}

fn indexer_loop(kernel: Weak<Kernel>, rx: Receiver<(String, Arc<Event>)>) {
    let mut batch: Vec<(String, Arc<Event>)> = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(x) => {
                batch.push(x);
                while let Ok(more) = rx.try_recv() {
                    batch.push(more);
                    if batch.len() >= 64 {
                        break;
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if batch.is_empty() {
            continue;
        }
        if let Some(k) = kernel.upgrade() {
            let cfg = k.cfg.read().clone();
            if let Ok(s) = k.plugins.get(&cfg.storage_plugin) {
                let events: Vec<Value> = batch
                    .iter()
                    .map(|(sess, e)| {
                        json!({
                            "session": sess,
                            "seq": e.seq,
                            "ts": e.ts_wall_ns,
                            "kind": e.kind.as_str(),
                            "payload": e.payload,
                        })
                    })
                    .collect();
                let _ = s.call("index.append", json!({"events": events}));
            }
        } else {
            break;
        }
        batch.clear();
    }
}

fn merge_json(base: Value, patch: Value) -> Value {
    match (base, patch) {
        (Value::Object(mut a), Value::Object(b)) => {
            for (k, v) in b {
                let cur = a.remove(&k).unwrap_or(Value::Null);
                a.insert(k, merge_json(cur, v));
            }
            Value::Object(a)
        }
        (_, b) => b,
    }
}

fn default_tools() -> Vec<Value> {
    vec![
        json!({"name": "tool.fs.read", "description": "读文件", "parameters": {"type": "object", "properties": {"path": {"type": "string"}}}}),
        json!({"name": "tool.fs.write", "description": "写文件", "parameters": {"type": "object", "properties": {"path": {"type": "string"}, "content": {"type": "string"}}}}),
        json!({"name": "tool.shell", "description": "执行 shell 命令", "parameters": {"type": "object", "properties": {"cmd": {"type": "string"}}}}),
    ]
}

/// 当前进程是否是新鲜拉起的（bench 用）。
pub fn uptime_ms(k: &Kernel) -> u128 {
    k.started.elapsed().as_millis()
}

/// 插件是否 Ready（doctor 用）。
pub fn plugin_ready(k: &Kernel, name: &str) -> bool {
    k.plugins
        .list()
        .iter()
        .any(|(n, st, _)| n == name && *st == PluginState::Ready)
}
