//! English UI variant generated from tui.rs; do not edit directly.
//! Regenerate: python3 scripts/gen-en-ui.py

//! `featheragent-tui`：三栏终端前端（§7.3）。
//!
//! 左：会话列表；中：对话流；右：上下文面板（预算分层 / 任务态 / 模型）；
//! 底：输入行。危险工具确认用 y/n 应答。所有数据走 UDS 契约，不读内核内存。

use std::io::{stdout, Stdout};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Terminal;
use serde_json::{json, Value};
use unicode_width::UnicodeWidthChar;

use featheragent::governance::paths::{Overrides, Paths};
use featheragent::server::client::{Client, Connect};
use featheragent::util::Args;

struct SettingsState {
    providers: Vec<String>,
    models: Vec<String>,
    provider_idx: usize,
    model_idx: usize,
    provider: String,
    base_url: String,
    model: String,
    api_key: String,
    temperature: String,
    context_window: String,
    max_turns: String,
    workspace_root: String,
    auto_allow_safe: bool,
    field: usize,
    status: String,
    models_hint: String,
    search_hint: String,
    /// 左侧栏：0 = 常规设置，之后是登记表里的插件。
    nav: Vec<Value>,
    nav_idx: usize,
    nav_focus: bool,
    detail: bool,
    /// 当前打开的插件页；内容由内核 `plugin.settings` 转述的声明驱动。
    plugin: Option<PluginFormState>,
}

/// 插件设置页：字段类型与取值全部来自内核转述，TUI 不认识具体插件。
struct PluginFormState {
    name: String,
    /// 声明的设置根路径（如 `review_gate` 或 `plugin_settings.<name>`）。
    root: String,
    title: String,
    description: String,
    /// 原始声明（`fields[].key/label/type/min/max/step/options/...`）。
    fields: Vec<Value>,
    /// 编辑中的值；类型与声明一致，字符串在 `text` 里暂存。
    values: Vec<Value>,
    text: Vec<String>,
    enabled: bool,
    enabled0: bool,
    /// `options_source == "models"` 的字段共用这一份本地模型列表（来自 provider.models）。
    models: Vec<String>,
    field: usize,
    status: String,
    dirty: bool,
}

/// 危险工具的确认请求：带完整参数，用户才能判断“这条命令该不该放”。
#[derive(Clone)]
struct ConfirmRequest {
    token: String,
    tool: String,
    risk: String,
    preview: String,
}

/// 把值转成可编辑文本；空值用空串，避免把 null 显示给用户。
fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// 按点号路径把值写进嵌套对象（`guide.model` → `{guide:{model:...}}`）。
fn set_nested(target: &mut serde_json::Map<String, Value>, path: &str, v: Value) {
    let parts: Vec<&str> = path.split('.').collect();
    let mut cur = target;
    for (i, part) in parts.iter().enumerate() {
        if i + 1 == parts.len() {
            cur.insert(part.to_string(), v);
            return;
        }
        let entry = cur
            .entry(part.to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(serde_json::Map::new());
        }
        cur = entry.as_object_mut().expect("New object");
    }
}

/// 插件页字段序：0 = 启用开关，1..=n = 声明字段，之后是保存/取消。
const PLUGIN_EXTRA: usize = 3;

impl PluginFormState {
    fn len(&self) -> usize {
        self.fields.len() + PLUGIN_EXTRA
    }
    fn save_idx(&self) -> usize {
        self.fields.len() + 1
    }
    fn cancel_idx(&self) -> usize {
        self.fields.len() + 2
    }
}

const SETTINGS_FIELDS: usize = 11;

fn default_provider_base(provider: &str) -> &'static str {
    match provider {
        "provider-ollama" => "http://127.0.0.1:11434",
        "provider-lmstudio" => "http://127.0.0.1:1234",
        "provider-vllm" => "http://127.0.0.1:8000",
        _ => "http://127.0.0.1:8080",
    }
}

fn is_default_provider_base(base: &str) -> bool {
    [
        "http://127.0.0.1:11434",
        "http://127.0.0.1:8080",
        "http://127.0.0.1:1234",
        "http://127.0.0.1:8000",
    ]
    .contains(&base)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Sessions,
    Chat,
}

struct PluginsState {
    items: Vec<Value>,
    selected: usize,
    status: String,
}

struct App {
    conn: Connect,
    sessions: Vec<Value>,
    selected: usize,
    messages: Vec<(String, String)>,
    session: Option<String>,
    state: String,
    ctx: Value,
    input: String,
    input_cursor: usize,
    focus: Focus,
    chat_scroll: usize,
    status: String,
    confirm: Option<ConfirmRequest>,
    settings: Option<SettingsState>,
    plugins: Option<PluginsState>,
    quit: bool,
}

fn main() {
    let args = Args::parse(std::env::args().skip(1));
    if args.has("--help") || args.positional.iter().any(|p| p == "-h") {
        println!("FeatherAgent TUI {}", env!("CARGO_PKG_VERSION"));
        println!("Usage: featheragent-tui-en [--root DIR] [--socket PATH]");
        return;
    }
    if args.has("version") {
        println!("FeatherAgent TUI {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("featheragent-tui"));
    let overrides = Overrides {
        install_root: args.flag("root").map(PathBuf::from),
        ..Default::default()
    };
    let paths = Paths::resolve(&exe, &overrides);
    let socket = args
        .flag("socket")
        .map(PathBuf::from)
        .unwrap_or_else(|| paths.socket());

    let conn = match Client::connect(&socket) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "Cannot connect to kernel {socket:?}: {e}\nStart `featheragent-en serve` first"
            );
            std::process::exit(1);
        }
    };
    if let Err(e) = conn
        .client
        .call_with_timeout("initialize", json!({}), Duration::from_secs(3))
    {
        eprintln!("Initialization failed (3s timeout / kernel not ready): {e}");
        std::process::exit(1);
    }

    let mut app = App {
        conn,
        sessions: Vec::new(),
        selected: 0,
        messages: Vec::new(),
        session: None,
        state: "idle".into(),
        ctx: Value::Null,
        input: String::new(),
        input_cursor: 0,
        focus: Focus::Sessions,
        chat_scroll: 0,
        status: format!(
            "Connected to {}; ↑↓ select session, Enter open",
            socket.display()
        ),
        confirm: None,
        settings: None,
        plugins: None,
        quit: false,
    };
    app.refresh_sessions();
    if let Some(id) = args.flag("session") {
        app.resume(id);
    }

    let mut terminal = match setup() {
        Ok(t) => t,
        Err(e) => {
            restore();
            eprintln!("TUI initialization failed: {e}");
            std::process::exit(1);
        }
    };
    let res = run(&mut terminal, &mut app);
    restore();
    if let Err(e) = res {
        eprintln!("TUI exited: {e}");
    }
}

fn setup() -> std::io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    Terminal::new(backend)
}

fn restore() {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), LeaveAlternateScreen);
}

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> std::io::Result<()> {
    while !app.quit {
        terminal.draw(|f| draw(f, app))?;
        app.drain_events();
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(k) = event::read()? {
                if k.kind == KeyEventKind::Press {
                    app.on_key(k.code, k.modifiers);
                }
            }
        }
    }
    Ok(())
}

impl App {
    fn refresh_sessions(&mut self) {
        if let Ok(v) = self.conn.client.call("session.list", json!({})) {
            self.sessions = v["sessions"].as_array().cloned().unwrap_or_default();
            if self.selected >= self.sessions.len() {
                self.selected = self.sessions.len().saturating_sub(1);
            }
        }
    }

    fn refresh_ctx(&mut self) {
        let Some(id) = &self.session else { return };
        if let Ok(v) = self.conn.client.call("ctx.inspect", json!({"session": id})) {
            self.state = v["state"].as_str().unwrap_or("idle").to_string();
            self.ctx = v["snapshot"].clone();
        }
    }

    fn resume(&mut self, id: &str) {
        match self
            .conn
            .client
            .call("session.resume", json!({"session": id}))
        {
            Ok(v) => {
                self.session = Some(id.to_string());
                self.state = v["state"].as_str().unwrap_or("idle").to_string();
                self.messages.clear();
                let ws = v
                    .get("workspace")
                    .and_then(|x| x.as_str())
                    .unwrap_or("default");
                self.messages.push((
                    "System".into(),
                    format!(
                        "Restored session {id} | project {ws} | {} events",
                        v["total"]
                    ),
                ));
                if let Some(events) = v.get("events").and_then(|x| x.as_array()) {
                    for e in events {
                        let kind = e.get("kind").and_then(|x| x.as_str()).unwrap_or("");
                        let payload = e.get("payload").cloned().unwrap_or(Value::Null);
                        if let Some(m) = event_message(kind, &payload) {
                            self.messages.push(m);
                        }
                    }
                }
                self.focus = Focus::Chat;
                self.chat_scroll = 0;
                self.input.clear();
                self.input_cursor = 0;
                self.status =
                    format!("Session {id}: ↑↓ scroll, Esc back to list, /help for commands");
                self.refresh_ctx();
            }
            Err(e) => self.status = format!("Restore failed: {e}"),
        }
    }

    fn create(&mut self, id: &str, workspace: &str) {
        let mut params = json!({"id": id});
        if !workspace.trim().is_empty() {
            params["workspace"] = json!(workspace.trim());
        }
        match self.conn.client.call("session.create", params) {
            Ok(_) => {
                self.session = Some(id.to_string());
                self.messages.clear();
                self.messages
                    .push(("System".into(), format!("New session {id}")));
                self.focus = Focus::Chat;
                self.chat_scroll = 0;
                self.input.clear();
                self.input_cursor = 0;
                self.status = format!("Session {id}: ↑↓ scroll, Esc back to list");
                self.refresh_sessions();
                self.refresh_ctx();
            }
            Err(e) => self.status = format!("Create failed: {e}"),
        }
    }

    fn refresh_plugins(&mut self) {
        let v = match self.conn.client.call("plugin.list", json!({})) {
            Ok(v) => v,
            Err(e) => {
                if let Some(p) = &mut self.plugins {
                    p.status = format!("Failed to read plugins: {e}");
                } else {
                    self.status = format!("Failed to read plugins: {e}");
                }
                return;
            }
        };
        let live: Vec<Value> = v
            .get("live")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        let mut items = Vec::new();
        for e in v
            .get("registry")
            .and_then(|x| x.as_array())
            .into_iter()
            .flatten()
        {
            let name = e.get("name").and_then(|x| x.as_str()).unwrap_or("?");
            let state = live
                .iter()
                .find(|x| x.get("name").and_then(|y| y.as_str()) == Some(name))
                .and_then(|x| x.get("state"))
                .and_then(|x| x.as_str())
                .unwrap_or("stopped");
            let mut item = e.clone();
            item["live_state"] = json!(state);
            items.push(item);
        }
        let status = format!(
            "{} plugins; Space toggles, r refreshes, Esc back",
            items.len()
        );
        if let Some(p) = &mut self.plugins {
            let old = p.selected.min(items.len().saturating_sub(1));
            p.items = items;
            p.selected = old;
            p.status = status;
        } else {
            let selected = 0;
            self.plugins = Some(PluginsState {
                items,
                selected,
                status,
            });
        }
    }

    fn toggle_plugin(&mut self) {
        let Some(p) = &self.plugins else { return };
        let Some(item) = p.items.get(p.selected) else {
            return;
        };
        let name = item
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let enabled = item
            .get("enabled")
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        let method = if enabled {
            "plugin.disable"
        } else {
            "plugin.enable"
        };
        match self.conn.client.call(method, json!({"name": name})) {
            Ok(_) => {
                self.refresh_plugins();
                if let Some(p) = &mut self.plugins {
                    p.status = format!(
                        "{} → {} (running process synced)",
                        name,
                        if enabled { "Disable" } else { "Enable" }
                    );
                }
            }
            Err(e) => {
                if let Some(p) = &mut self.plugins {
                    p.status = format!("Toggle failed: {e}");
                }
            }
        }
    }

    fn on_plugins_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => {
                self.plugins = None;
                self.status = "Plugin management closed".into();
            }
            KeyCode::Up => {
                if let Some(p) = &mut self.plugins {
                    p.selected = p.selected.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if let Some(p) = &mut self.plugins {
                    if p.selected + 1 < p.items.len() {
                        p.selected += 1;
                    }
                }
            }
            KeyCode::Char(' ') | KeyCode::Enter => self.toggle_plugin(),
            KeyCode::Char('r') => self.refresh_plugins(),
            _ => {}
        }
    }

    fn open_settings(&mut self) {
        let cfg = match self.conn.client.call("config.get", json!({})) {
            Ok(v) => v,
            Err(e) => {
                self.status = format!("Failed to read configuration: {e}");
                return;
            }
        };
        let mut providers: Vec<String> = Vec::new();
        if let Ok(v) = self.conn.client.call("plugin.list", json!({})) {
            let mut add = |name: &str, category: &str| {
                if (category.contains("provider") || name.starts_with("provider-"))
                    && !providers.iter().any(|x| x == name)
                {
                    providers.push(name.to_string());
                }
            };
            if let Some(arr) = v.get("registry").and_then(|x| x.as_array()) {
                for p in arr {
                    add(
                        p.get("name").and_then(|x| x.as_str()).unwrap_or(""),
                        p.get("category").and_then(|x| x.as_str()).unwrap_or(""),
                    );
                }
            }
            if let Some(arr) = v.get("live").and_then(|x| x.as_array()) {
                for p in arr {
                    add(p.get("name").and_then(|x| x.as_str()).unwrap_or(""), "");
                }
            }
        }
        let provider = cfg
            .get("provider_plugin")
            .and_then(|x| x.as_str())
            .unwrap_or("provider-mock")
            .to_string();
        if !providers.iter().any(|x| x == &provider) {
            providers.push(provider.clone());
        }
        providers.sort();
        let model = cfg
            .get("model")
            .and_then(|x| x.as_str())
            .unwrap_or("qwen2.5:7b")
            .to_string();
        let provider_idx = providers.iter().position(|x| x == &provider).unwrap_or(0);
        let state = SettingsState {
            providers,
            models: Vec::new(),
            provider_idx,
            model_idx: 0,
            provider,
            base_url: cfg
                .get("base_url")
                .and_then(|x| x.as_str())
                .unwrap_or("http://127.0.0.1:11434")
                .to_string(),
            model,
            api_key: cfg
                .get("provider_api_key")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            temperature: format!(
                "{:.2}",
                cfg.get("temperature")
                    .and_then(|x| x.as_f64())
                    .unwrap_or(0.7)
            ),
            context_window: cfg
                .get("context_window")
                .and_then(|x| x.as_u64())
                .unwrap_or(8192)
                .to_string(),
            max_turns: cfg
                .get("max_turns")
                .and_then(|x| x.as_u64())
                .unwrap_or(6)
                .to_string(),
            workspace_root: cfg
                .get("workspace_root")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            auto_allow_safe: cfg
                .get("auto_allow_safe")
                .and_then(|x| x.as_bool())
                .unwrap_or(true),
            field: 2,
            status: "Configuration loaded; ←/↑↓ sidebar, Tab form, Ctrl+S save".into(),
            models_hint: "Reading model list…".into(),
            search_hint: match self.conn.client.call("search.status", json!({})) {
                Ok(v) => {
                    let p = v["provider"].as_str().unwrap_or("?");
                    if p == "local" {
                        if v["configured"].as_bool().unwrap_or(false) {
                            "Web: local · public web · free, no key".to_string()
                        } else {
                            "Web: local · plugin not ready".to_string()
                        }
                    } else if p == "none" {
                        "Web: off".to_string()
                    } else {
                        format!(
                            "Web: {} · {} · {}",
                            p,
                            v["key_env"].as_str().unwrap_or("?"),
                            if v["configured"].as_bool().unwrap_or(false) {
                                "Configured"
                            } else {
                                "Not configured"
                            }
                        )
                    }
                }
                Err(e) => format!("Failed to read web-search status: {e}"),
            },
            nav: Vec::new(),
            nav_idx: 0,
            nav_focus: true,
            detail: false,
            plugin: None,
        };
        self.settings = Some(state);
        self.refresh_settings_plugins();
        self.refresh_settings_models(false);
    }

    fn refresh_settings_plugins(&mut self) {
        let v = match self.conn.client.call("plugin.list", json!({})) {
            Ok(v) => v,
            Err(e) => {
                if let Some(s) = &mut self.settings {
                    s.status = format!("Failed to read plugin sidebar: {e}");
                }
                return;
            }
        };
        let live: Vec<Value> = v
            .get("live")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        let mut nav = Vec::new();
        for e in v
            .get("registry")
            .and_then(|x| x.as_array())
            .into_iter()
            .flatten()
        {
            let name = e.get("name").and_then(|x| x.as_str()).unwrap_or("?");
            let live_state = live
                .iter()
                .find(|x| x.get("name").and_then(|y| y.as_str()) == Some(name))
                .and_then(|x| x.get("state"))
                .and_then(|x| x.as_str())
                .unwrap_or("stopped");
            let mut item = e.clone();
            item["live_state"] = json!(live_state);
            nav.push(item);
        }
        nav.sort_by(|a, b| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .cmp(b["name"].as_str().unwrap_or(""))
        });
        if let Some(s) = &mut self.settings {
            s.nav_idx = s.nav_idx.min(nav.len().saturating_sub(1));
            s.nav = nav;
        }
    }

    fn toggle_settings_plugin(&mut self) {
        let (name, enabled) = {
            let Some(s) = &self.settings else { return };
            let Some(item) = s.nav.get(s.nav_idx.saturating_sub(1)) else {
                return;
            };
            (
                item.get("name")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                item.get("enabled")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false),
            )
        };
        if name.is_empty() {
            return;
        }
        let method = if enabled {
            "plugin.disable"
        } else {
            "plugin.enable"
        };
        match self.conn.client.call(method, json!({"name": name})) {
            Ok(_) => {
                self.refresh_settings_plugins();
                if let Some(s) = &mut self.settings {
                    s.status = format!(
                        "{name} → {} (running process synced)",
                        if enabled { "Disable" } else { "Enable" }
                    );
                }
            }
            Err(e) => {
                if let Some(s) = &mut self.settings {
                    s.status = format!("Toggle failed: {e}");
                }
            }
        }
    }

    fn refresh_settings_models(&mut self, prefer_first: bool) {
        let (provider, base_url, model, api_key) = match &self.settings {
            Some(s) => (
                s.provider.clone(),
                s.base_url.clone(),
                s.model.clone(),
                s.api_key.clone(),
            ),
            None => return,
        };
        let (models, hint) = match self.conn.client.call(
            "provider.models",
            json!({"provider": provider, "base_url": base_url, "model": model, "api_key": api_key}),
        ) {
            Ok(v) => {
                let models: Vec<String> = v
                    .get("models")
                    .and_then(|x| x.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect();
                let hint = match v.get("error").and_then(|x| x.as_str()) {
                    Some(e) => format!("Model list unavailable (manual entry allowed): {e}"),
                    None => format!("{} models; ←/→ select, or type directly", models.len()),
                };
                (models, hint)
            }
            Err(e) => (
                Vec::new(),
                format!("Failed to read model list (manual entry allowed): {e}"),
            ),
        };
        if let Some(s) = &mut self.settings {
            s.models = models;
            if prefer_first && !s.models.is_empty() && !s.models.iter().any(|x| x == &s.model) {
                s.model = s.models[0].clone();
            }
            s.model_idx = s.models.iter().position(|x| x == &s.model).unwrap_or(0);
            s.models_hint = hint;
        }
    }

    fn save_settings(&mut self) {
        let (
            provider,
            base_url,
            model,
            api_key,
            temperature,
            context_window,
            max_turns,
            workspace_root,
            auto_allow_safe,
        ) = match &self.settings {
            Some(s) => (
                s.provider.clone(),
                s.base_url.trim().to_string(),
                s.model.trim().to_string(),
                s.api_key.trim().to_string(),
                s.temperature.trim().parse::<f64>(),
                s.context_window.trim().parse::<u64>(),
                s.max_turns.trim().parse::<u64>(),
                s.workspace_root.trim().to_string(),
                s.auto_allow_safe,
            ),
            None => return,
        };
        let (Ok(temperature), Ok(context_window), Ok(max_turns)) =
            (temperature, context_window, max_turns)
        else {
            if let Some(s) = &mut self.settings {
                s.status = "Temperature/context/turns must be numbers".into();
            }
            return;
        };
        if base_url.is_empty() || model.is_empty() {
            if let Some(s) = &mut self.settings {
                s.status = "Base URL and model name must not be empty".into();
            }
            return;
        }
        if !(0.0..=2.0).contains(&temperature) {
            if let Some(s) = &mut self.settings {
                s.status = "Temperature must be between 0.00 and 2.00".into();
            }
            return;
        }
        let patch = json!({
            "provider_plugin": provider,
            "base_url": base_url,
            "model": model,
            "provider_api_key": api_key,
            "temperature": temperature,
            "context_window": context_window,
            "max_turns": max_turns,
            "workspace_root": workspace_root,
            "auto_allow_safe": auto_allow_safe,
        });
        match self.conn.client.call("config.set", json!({"patch": patch})) {
            Ok(v) => {
                let cfg = v.get("config").cloned().unwrap_or(Value::Null);
                if let Some(s) = &mut self.settings {
                    s.status = format!(
                        "Saved and applied: {} / {} / T={:.2}; workspace changes apply after restarting serve",
                        provider, model, temperature
                    );
                    if let Some(t) = cfg.get("temperature").and_then(|x| x.as_f64()) {
                        s.temperature = format!("{t:.2}");
                    }
                }
                self.refresh_ctx();
            }
            Err(e) => {
                if let Some(s) = &mut self.settings {
                    s.status = format!("Save failed: {e}");
                }
            }
        }
    }

    fn on_settings_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        if mods.contains(KeyModifiers::CONTROL)
            && matches!(code, KeyCode::Char('s') | KeyCode::Char('S'))
        {
            if self.settings.as_ref().is_some_and(|s| s.plugin.is_some()) {
                self.save_plugin_settings();
            } else {
                self.save_settings();
            }
            return;
        }
        let (nav_idx, nav_focus, detail, in_plugin) = match &self.settings {
            Some(s) => (s.nav_idx, s.nav_focus, s.detail, s.plugin.is_some()),
            None => return,
        };
        if detail {
            if in_plugin {
                self.on_plugin_key(code);
                return;
            }
            match code {
                KeyCode::Esc => {
                    if let Some(s) = &mut self.settings {
                        s.detail = false;
                        s.nav_focus = true;
                    }
                }
                KeyCode::Char(' ') | KeyCode::Enter => self.toggle_settings_plugin(),
                KeyCode::Char('r') => self.refresh_settings_plugins(),
                _ => {}
            }
            return;
        }
        if nav_focus {
            match code {
                KeyCode::Up => {
                    if let Some(s) = &mut self.settings {
                        s.nav_idx = s.nav_idx.saturating_sub(1);
                    }
                }
                KeyCode::Down => {
                    if let Some(s) = &mut self.settings {
                        if s.nav_idx + 1 < s.nav.len() + 1 {
                            s.nav_idx += 1;
                        }
                    }
                }
                KeyCode::Enter | KeyCode::Tab => {
                    if nav_idx == 0 {
                        if let Some(s) = &mut self.settings {
                            s.nav_focus = false;
                            s.field = 0;
                        }
                    } else {
                        self.open_plugin_page();
                    }
                }
                KeyCode::Char('r') => self.refresh_settings_plugins(),
                KeyCode::Esc => {
                    self.settings = None;
                    self.status = "Settings closed (unsaved changes discarded)".into();
                }
                _ => {}
            }
            return;
        }
        let field = match &self.settings {
            Some(s) => s.field,
            None => return,
        };
        if mods.contains(KeyModifiers::CONTROL)
            && matches!(code, KeyCode::Char('u') | KeyCode::Char('U'))
            && field == 3
        {
            if let Some(s) = &mut self.settings {
                s.api_key.clear();
                s.status = "API Key cleared; press Ctrl+S to apply".into();
            }
            return;
        }
        match code {
            KeyCode::Esc => {
                if let Some(s) = &mut self.settings {
                    s.nav_focus = true;
                    s.detail = false;
                }
            }
            KeyCode::Enter => {
                if field == SETTINGS_FIELDS - 1 {
                    self.settings = None;
                } else {
                    self.save_settings();
                }
            }
            KeyCode::Tab => {
                if let Some(s) = &mut self.settings {
                    s.field = (s.field + 1) % SETTINGS_FIELDS;
                }
            }
            KeyCode::BackTab => {
                if let Some(s) = &mut self.settings {
                    s.field = (s.field + SETTINGS_FIELDS - 1) % SETTINGS_FIELDS;
                }
            }
            KeyCode::Up => {
                if let Some(s) = &mut self.settings {
                    s.field = s.field.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if let Some(s) = &mut self.settings {
                    s.field = (s.field + 1) % SETTINGS_FIELDS;
                }
            }
            KeyCode::Left | KeyCode::Right => {
                let dir: i32 = if matches!(code, KeyCode::Left) { -1 } else { 1 };
                let mut refresh = false;
                if let Some(s) = &mut self.settings {
                    match field {
                        0 if !s.providers.is_empty() => {
                            let n = s.providers.len() as i32;
                            s.provider_idx = ((s.provider_idx as i32 + dir + n) % n) as usize;
                            s.provider = s.providers[s.provider_idx].clone();
                            if s.base_url.trim().is_empty()
                                || is_default_provider_base(s.base_url.trim())
                            {
                                s.base_url = default_provider_base(&s.provider).to_string();
                            }
                            refresh = true;
                        }
                        2 if !s.models.is_empty() => {
                            let n = s.models.len() as i32;
                            s.model_idx = ((s.model_idx as i32 + dir + n) % n) as usize;
                            s.model = s.models[s.model_idx].clone();
                        }
                        4 => {
                            let v = s.temperature.parse::<f64>().unwrap_or(0.7);
                            s.temperature =
                                format!("{:.2}", (v + dir as f64 * 0.05).clamp(0.0, 2.0));
                        }
                        5 => {
                            let v = s.context_window.parse::<u64>().unwrap_or(8192) as i64;
                            s.context_window =
                                (v + dir as i64 * 256).clamp(512, 131072).to_string();
                        }
                        6 => {
                            let v = s.max_turns.parse::<i64>().unwrap_or(6);
                            s.max_turns = (v + dir as i64).clamp(1, 32).to_string();
                        }
                        _ => {}
                    }
                }
                if refresh {
                    self.refresh_settings_models(true);
                }
            }
            KeyCode::Backspace => {
                if let Some(s) = &mut self.settings {
                    let target = match field {
                        1 => Some(&mut s.base_url),
                        2 => Some(&mut s.model),
                        3 => Some(&mut s.api_key),
                        4 => Some(&mut s.temperature),
                        5 => Some(&mut s.context_window),
                        6 => Some(&mut s.max_turns),
                        7 => Some(&mut s.workspace_root),
                        _ => None,
                    };
                    if let Some(t) = target {
                        t.pop();
                    }
                }
            }
            KeyCode::Char(' ') if field == 8 => {
                if let Some(s) = &mut self.settings {
                    s.auto_allow_safe = !s.auto_allow_safe;
                }
            }
            KeyCode::Char(c) => {
                if let Some(s) = &mut self.settings {
                    let target = match field {
                        1 => Some(&mut s.base_url),
                        2 => Some(&mut s.model),
                        3 => Some(&mut s.api_key),
                        4 => Some(&mut s.temperature),
                        5 => Some(&mut s.context_window),
                        6 => Some(&mut s.max_turns),
                        7 => Some(&mut s.workspace_root),
                        _ => None,
                    };
                    if let Some(t) = target {
                        t.push(c);
                    }
                }
            }
            _ => {}
        }
    }

    /// 打开插件页：内核用 `plugin.settings` 转述插件声明；未声明时退回只读详情。
    fn open_plugin_page(&mut self) {
        let name = match &self.settings {
            Some(s) if s.nav_idx > 0 => s.nav[s.nav_idx - 1]
                .get("name")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            _ => return,
        };
        let enabled = self
            .settings
            .as_ref()
            .and_then(|s| s.nav.get(s.nav_idx - 1))
            .and_then(|v| v.get("enabled"))
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        match self
            .conn
            .client
            .call("plugin.settings", json!({"name": name}))
        {
            Ok(v) => {
                let fields: Vec<Value> = v
                    .get("fields")
                    .and_then(|x| x.as_array())
                    .cloned()
                    .unwrap_or_default();
                let values: Vec<Value> = fields
                    .iter()
                    .map(|f| {
                        v.get("values")
                            .and_then(|x| {
                                x.get(f.get("key").and_then(|k| k.as_str()).unwrap_or(""))
                            })
                            .cloned()
                            .unwrap_or_else(|| f.get("default").cloned().unwrap_or(Value::Null))
                    })
                    .collect();
                let text: Vec<String> = values.iter().map(value_text).collect();
                // 模型类字段：按声明向本地模型服务要一份列表，前端只负责展示。
                let wants_models = fields
                    .iter()
                    .any(|f| f.get("options_source").and_then(|x| x.as_str()) == Some("models"));
                let (models, model_hint) = if wants_models {
                    match self.conn.client.call("provider.models", json!({})) {
                        Ok(mv) => {
                            let list: Vec<String> = mv
                                .get("models")
                                .and_then(|x| x.as_array())
                                .into_iter()
                                .flatten()
                                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                                .collect();
                            let hint = match mv.get("error").and_then(|x| x.as_str()) {
                                Some(e) => {
                                    format!("Model list unavailable (manual entry allowed): {e}")
                                }
                                None => format!("Local models: {}; ←/→ select", list.len()),
                            };
                            (list, hint)
                        }
                        Err(e) => (
                            Vec::new(),
                            format!("Failed to read model list (manual entry allowed): {e}"),
                        ),
                    }
                } else {
                    (Vec::new(), String::new())
                };
                let title = v
                    .get("title")
                    .and_then(|x| x.as_str())
                    .unwrap_or(&name)
                    .to_string();
                let description = v
                    .get("description")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                if let Some(s) = &mut self.settings {
                    s.detail = true;
                    s.nav_focus = false;
                    s.plugin = Some(PluginFormState {
                        name: name.clone(),
                        root: v
                            .get("root")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                        title,
                        description,
                        fields,
                        values,
                        text,
                        enabled,
                        enabled0: enabled,
                        models,
                        field: 0,
                        status: format!(
                            "Read {name} declaration; Enter saves{}",
                            if model_hint.is_empty() {
                                String::new()
                            } else {
                                format!("；{model_hint}")
                            }
                        ),
                        dirty: false,
                    });
                }
            }
            Err(e) => {
                // 插件没有声明设置项：保留原来的只读详情页（启停 + 元信息）。
                if let Some(s) = &mut self.settings {
                    s.detail = true;
                    s.plugin = None;
                    s.status = format!("{name}: {e}");
                }
            }
        }
    }

    /// 插件页按键：字段 0 是启用开关，其余字段按内核声明的类型调整。
    fn on_plugin_key(&mut self, code: KeyCode) {
        let (n, _save_idx, cancel_idx, field) = match &self.settings {
            Some(s) => match &s.plugin {
                Some(p) => (p.len(), p.save_idx(), p.cancel_idx(), p.field),
                None => return,
            },
            None => return,
        };
        if code == KeyCode::Esc {
            if let Some(s) = &mut self.settings {
                s.plugin = None;
                s.detail = false;
                s.nav_focus = true;
                s.status = "Back to sidebar (unsaved changes discarded)".into();
            }
            return;
        }
        if code == KeyCode::Enter {
            if field == cancel_idx {
                if let Some(s) = &mut self.settings {
                    s.plugin = None;
                    s.detail = false;
                    s.nav_focus = true;
                }
            } else if field == 0 {
                // 启停行：Enter 先翻转再落盘，语义与“改完就保存”一致。
                if let Some(s) = &mut self.settings {
                    if let Some(p) = &mut s.plugin {
                        p.enabled = !p.enabled;
                        p.dirty = true;
                    }
                }
                self.save_plugin_settings();
            } else {
                // rc.3 复测 I4：以前只有光标停在“保存”上才有动作，回车等于没反应。
                // 现在任意字段 Enter 都提交整个表单（含保存行本身）。
                self.save_plugin_settings();
            }
            return;
        }
        match code {
            KeyCode::Up => {
                if let Some(s) = &mut self.settings {
                    if let Some(p) = &mut s.plugin {
                        p.field = (p.field + n - 1) % n;
                    }
                }
            }
            KeyCode::Down | KeyCode::Tab => {
                if let Some(s) = &mut self.settings {
                    if let Some(p) = &mut s.plugin {
                        p.field = (p.field + 1) % n;
                    }
                }
            }
            KeyCode::BackTab => {
                if let Some(s) = &mut self.settings {
                    if let Some(p) = &mut s.plugin {
                        p.field = (p.field + n - 1) % n;
                    }
                }
            }
            KeyCode::Left | KeyCode::Right => {
                let dir: i32 = if matches!(code, KeyCode::Left) { -1 } else { 1 };
                self.adjust_plugin_field(field, dir);
            }
            KeyCode::Char(' ') => {
                if field == 0 {
                    if let Some(s) = &mut self.settings {
                        if let Some(p) = &mut s.plugin {
                            p.enabled = !p.enabled;
                            p.dirty = true;
                        }
                    }
                } else {
                    self.adjust_plugin_field(field, 1);
                }
            }
            KeyCode::Backspace => {
                if field >= 1 && field <= n - PLUGIN_EXTRA {
                    if let Some(s) = &mut self.settings {
                        if let Some(p) = &mut s.plugin {
                            p.text[field - 1].pop();
                            p.dirty = true;
                        }
                    }
                }
            }
            KeyCode::Char(c) => {
                if let Some(s) = &mut self.settings {
                    if let Some(p) = &mut s.plugin {
                        if (1..=n - PLUGIN_EXTRA).contains(&field) {
                            let kind = p.fields[field - 1]
                                .get("type")
                                .and_then(|x| x.as_str())
                                .unwrap_or("string");
                            if matches!(kind, "string" | "int" | "number") {
                                p.text[field - 1].push(c);
                                p.dirty = true;
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// 按声明类型调整一个字段：bool 翻转，enum 循环，int/number 按步进。
    fn adjust_plugin_field(&mut self, field: usize, dir: i32) {
        if let Some(s) = &mut self.settings {
            if let Some(p) = &mut s.plugin {
                if field == 0 {
                    p.enabled = !p.enabled;
                    p.dirty = true;
                    return;
                }
                let Some(f) = p.fields.get(field - 1).cloned() else {
                    return;
                };
                let kind = f.get("type").and_then(|x| x.as_str()).unwrap_or("string");
                let idx = field - 1;
                match kind {
                    "bool" => {
                        let cur = p.values[idx].as_bool().unwrap_or(false);
                        p.values[idx] = json!(!cur);
                    }
                    "enum" => {
                        let opts: Vec<String> = f
                            .get("options")
                            .and_then(|x| x.as_array())
                            .map(|a| {
                                a.iter()
                                    .filter_map(|o| o.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();
                        if !opts.is_empty() {
                            let cur = p.values[idx].as_str().unwrap_or("").to_string();
                            let pos = opts.iter().position(|o| *o == cur).unwrap_or(0) as i32;
                            let next = ((pos + dir).rem_euclid(opts.len() as i32)) as usize;
                            p.values[idx] = json!(opts[next]);
                            p.text[idx] = opts[next].clone();
                        }
                    }
                    "int" => {
                        let step = f.get("step").and_then(|x| x.as_i64()).unwrap_or(1);
                        let mut v = p.text[idx].trim().parse::<i64>().unwrap_or_else(|_| {
                            f.get("default").and_then(|x| x.as_i64()).unwrap_or(0)
                        });
                        v += dir as i64 * step;
                        if let Some(m) = f.get("min").and_then(|x| x.as_f64()) {
                            v = v.max(m as i64);
                        }
                        if let Some(m) = f.get("max").and_then(|x| x.as_f64()) {
                            v = v.min(m as i64);
                        }
                        p.text[idx] = v.to_string();
                    }
                    "number" => {
                        let step = f.get("step").and_then(|x| x.as_f64()).unwrap_or(0.05);
                        let mut v = p.text[idx].trim().parse::<f64>().unwrap_or_else(|_| {
                            f.get("default").and_then(|x| x.as_f64()).unwrap_or(0.0)
                        });
                        v = (v + dir as f64 * step).max(0.0);
                        if let Some(m) = f.get("min").and_then(|x| x.as_f64()) {
                            v = v.max(m);
                        }
                        if let Some(m) = f.get("max").and_then(|x| x.as_f64()) {
                            v = v.min(m);
                        }
                        p.text[idx] = format!("{v:.2}");
                    }
                    "string" => {
                        // options_source == "models" 的字段循环「跟随当前模型 + 本地模型列表」。
                        let source = f
                            .get("options_source")
                            .and_then(|x| x.as_str())
                            .unwrap_or("");
                        if source == "models" {
                            let mut opts: Vec<String> = vec![String::new()];
                            opts.extend(p.models.iter().cloned());
                            if opts.len() > 1 {
                                let cur = p.text[idx].clone();
                                let pos = opts.iter().position(|o| *o == cur).unwrap_or(0) as i32;
                                let next = ((pos + dir).rem_euclid(opts.len() as i32)) as usize;
                                let chosen = opts[next].clone();
                                p.text[idx] = chosen.clone();
                                p.values[idx] = json!(chosen);
                            }
                        }
                    }
                    _ => {}
                }
                p.dirty = true;
            }
        }
    }

    /// 保存插件页：只提交改动的声明字段；启用状态走 plugin.enable/disable。
    fn save_plugin_settings(&mut self) {
        let (name, values, text, fields, enabled, enabled0) = match &self.settings {
            Some(s) => match &s.plugin {
                Some(p) => (
                    p.name.clone(),
                    p.values.clone(),
                    p.text.clone(),
                    p.fields.clone(),
                    p.enabled,
                    p.enabled0,
                ),
                None => return,
            },
            None => return,
        };
        // 声明字段 → 值；数字/字符串以编辑中的文本为准，解析失败即报错。
        let mut patch = serde_json::Map::new();
        for (i, f) in fields.iter().enumerate() {
            let kind = f.get("type").and_then(|x| x.as_str()).unwrap_or("string");
            let v = match kind {
                "bool" => values[i].clone(),
                "enum" => values[i].clone(),
                "int" => match text[i].trim().parse::<i64>() {
                    Ok(n) => json!(n),
                    Err(_) => {
                        if let Some(s) = &mut self.settings {
                            if let Some(p) = &mut s.plugin {
                                p.status = format!(
                                    "{} must be an integer",
                                    f["label"].as_str().unwrap_or("this field")
                                );
                            }
                        }
                        return;
                    }
                },
                "number" => match text[i].trim().parse::<f64>() {
                    Ok(n) => json!(n),
                    Err(_) => {
                        if let Some(s) = &mut self.settings {
                            if let Some(p) = &mut s.plugin {
                                p.status = format!(
                                    "{} must be a number",
                                    f["label"].as_str().unwrap_or("this field")
                                );
                            }
                        }
                        return;
                    }
                },
                _ => json!(text[i].clone()),
            };
            let path = f
                .get("path")
                .and_then(|x| x.as_str())
                .filter(|p| !p.trim().is_empty())
                .map(|p| p.to_string())
                .unwrap_or_else(|| {
                    let root = match &self.settings {
                        Some(s) => match &s.plugin {
                            Some(p) => p.root.clone(),
                            None => String::new(),
                        },
                        None => String::new(),
                    };
                    let key = f.get("key").and_then(|x| x.as_str()).unwrap_or("");
                    if root.is_empty() {
                        key.to_string()
                    } else {
                        format!("{root}.{key}")
                    }
                });
            set_nested(&mut patch, &path, v);
        }
        let ok = if patch.is_empty() {
            true
        } else {
            match self
                .conn
                .client
                .call("config.set", json!({"patch": Value::Object(patch)}))
            {
                Ok(_) => true,
                Err(e) => {
                    if let Some(s) = &mut self.settings {
                        if let Some(p) = &mut s.plugin {
                            p.status = format!("Save failed: {e}");
                        }
                    }
                    false
                }
            }
        };
        if !ok {
            return;
        }
        if enabled != enabled0 {
            let method = if enabled {
                "plugin.enable"
            } else {
                "plugin.disable"
            };
            if let Err(e) = self.conn.client.call(method, json!({"name": name})) {
                if let Some(s) = &mut self.settings {
                    if let Some(p) = &mut s.plugin {
                        p.status = format!("Settings saved, but toggle failed: {e}");
                    }
                }
                return;
            }
        }
        if let Some(s) = &mut self.settings {
            if let Some(p) = &mut s.plugin {
                p.status = format!(
                    "Saved settings for {name}{}",
                    if enabled != enabled0 {
                        if enabled {
                            ", now enabled"
                        } else {
                            ", now disabled"
                        }
                    } else {
                        ""
                    }
                );
                p.enabled0 = enabled;
                p.dirty = false;
            }
        }
        self.refresh_settings_plugins();
        self.refresh_ctx();
    }

    fn input_len(&self) -> usize {
        self.input.chars().count()
    }

    fn input_byte_at_cursor(&self) -> usize {
        self.input
            .char_indices()
            .nth(self.input_cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.input.len())
    }

    fn insert_input(&mut self, c: char) {
        let at = self.input_byte_at_cursor();
        self.input.insert(at, c);
        self.input_cursor += 1;
    }

    fn backspace_input(&mut self) {
        if self.input_cursor == 0 {
            return;
        }
        let prev = self.input_cursor - 1;
        if let Some((at, _)) = self.input.char_indices().nth(prev) {
            self.input.remove(at);
            self.input_cursor = prev;
        }
    }

    fn delete_input(&mut self) {
        if self.input_cursor >= self.input_len() {
            return;
        }
        if let Some((at, _)) = self.input.char_indices().nth(self.input_cursor) {
            self.input.remove(at);
        }
    }

    fn move_input_cursor(&mut self, delta: isize) {
        let max = self.input_len();
        self.input_cursor = if delta < 0 {
            self.input_cursor.saturating_sub((-delta) as usize)
        } else {
            (self.input_cursor + delta as usize).min(max)
        };
    }

    fn on_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        // 危险工具确认优先
        if let Some(c) = self.confirm.clone() {
            match code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    let _ = self
                        .conn
                        .client
                        .call("tool.confirm", json!({"token": c.token, "allow": true}));
                    self.status = format!("Allowed {}", c.tool);
                    self.confirm = None;
                    return;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    let _ = self
                        .conn
                        .client
                        .call("tool.deny", json!({"token": c.token}));
                    self.status = format!("Denied {}", c.tool);
                    self.confirm = None;
                    return;
                }
                _ => {}
            }
            self.status = format!("Pending ({}): {} — y allow / n deny", c.risk, c.tool);
            return;
        }
        if self.settings.is_some() {
            self.on_settings_key(code, mods);
            return;
        }
        if self.plugins.is_some() {
            self.on_plugins_key(code);
            return;
        }
        match code {
            KeyCode::Char('c') if mods.contains(KeyModifiers::CONTROL) => self.quit = true,
            KeyCode::Esc => {
                if self.focus == Focus::Chat {
                    // 对话内 Esc 只退回会话列表，不退出 TUI；再次 Esc 才退出。
                    self.focus = Focus::Sessions;
                    self.chat_scroll = 0;
                    self.input.clear();
                    self.input_cursor = 0;
                    self.status = "Sessions: ↑↓ select, Enter open, Esc quit".into();
                } else {
                    self.quit = true;
                }
            }
            KeyCode::Up => {
                if self.focus == Focus::Chat {
                    self.chat_scroll = self.chat_scroll.saturating_add(1);
                } else {
                    self.selected = self.selected.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if self.focus == Focus::Chat {
                    self.chat_scroll = self.chat_scroll.saturating_sub(1);
                } else if self.selected + 1 < self.sessions.len() {
                    self.selected += 1;
                }
            }
            KeyCode::PageUp if self.focus == Focus::Chat => {
                self.chat_scroll = self.chat_scroll.saturating_add(10);
            }
            KeyCode::PageDown if self.focus == Focus::Chat => {
                self.chat_scroll = self.chat_scroll.saturating_sub(10);
            }
            KeyCode::Left => self.move_input_cursor(-1),
            KeyCode::Right => self.move_input_cursor(1),
            KeyCode::Home => self.input_cursor = 0,
            KeyCode::End => self.input_cursor = self.input_len(),
            KeyCode::Backspace => self.backspace_input(),
            KeyCode::Delete => self.delete_input(),
            KeyCode::Enter => {
                let text = self.input.trim().to_string();
                if text.is_empty() {
                    if self.focus == Focus::Sessions {
                        if let Some(s) = self
                            .sessions
                            .get(self.selected)
                            .and_then(|x| x["id"].as_str())
                            .map(|x| x.to_string())
                        {
                            self.resume(&s);
                        }
                    }
                    return;
                }
                self.input.clear();
                self.input_cursor = 0;
                if let Some(cmd) = text.strip_prefix('/') {
                    self.command(cmd);
                } else if self.focus == Focus::Chat {
                    self.send(&text);
                } else {
                    self.status = "Press Enter to open a session before typing".into();
                }
            }
            KeyCode::Char(c) => self.insert_input(c),
            _ => {}
        }
    }

    fn command(&mut self, cmd: &str) {
        let mut it = cmd.split_whitespace();
        let name = it.next().unwrap_or("");
        let arg = it.next().unwrap_or("");
        let rest = it.collect::<Vec<_>>().join(" ");
        match name {
            "quit" | "q" => self.quit = true,
            "new" => {
                let id = if arg.is_empty() { "s-tui" } else { arg };
                self.create(id, &rest);
            }
            "use" | "resume" => {
                if arg.is_empty() {
                    self.status = "Usage: /use <session>".into();
                } else {
                    self.resume(arg);
                }
            }
            "workspace" | "project" => {
                if arg.is_empty() {
                    self.status = "Usage: /workspace <project>; projects live one level under workspace_root".into();
                } else {
                    match self
                        .conn
                        .client
                        .call("workspace.create", json!({"name": arg}))
                    {
                        Ok(v) => self.status = format!(
                            "Created project {} ({}); /new <session> {} can bind",
                            v["name"].as_str().unwrap_or(arg),
                            v["path"].as_str().unwrap_or(""),
                            arg
                        ),
                        Err(e) => self.status = format!("Project creation failed: {e}"),
                    }
                }
            }
            "workspaces" => match self.conn.client.call("workspace.list", json!({})) {
                Ok(v) => {
                    let items = v["workspaces"].as_array().cloned().unwrap_or_default();
                    let text = items
                        .iter()
                        .map(|w| {
                            format!(
                                "{}  {}",
                                w["name"].as_str().unwrap_or("?"),
                                if w["has_agent"].as_bool().unwrap_or(false) { "(.featheagent)" } else { "" }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.messages.push((
                        "System".into(),
                        format!("Project container {}\n{text}", v["root"].as_str().unwrap_or("")),
                    ));
                }
                Err(e) => self.status = format!("Failed to read project: {e}"),
            },
            "fork" => {
                let Some(id) = self.session.clone() else {
                    self.status = "Use /new or /use first".into();
                    return;
                };
                let target = if arg.is_empty() {
                    format!("{id}-fork")
                } else {
                    arg.to_string()
                };
                match self
                    .conn
                    .client
                    .call("session.fork", json!({"session": id, "as": target}))
                {
                    Ok(v) => {
                        self.status = format!("Forked → {target} (copied {} events)", v["copied"]);
                        self.refresh_sessions();
                    }
                    Err(e) => self.status = format!("fork failed: {e}"),
                }
            }
            "stop" | "cancel" => {
                if let Some(id) = &self.session {
                    match self.conn.client.call("chat.stop", json!({"session": id})) {
                        Ok(_) => self.status = "Emergency stop complete; generated content and context are saved in the session log".into(),
                        Err(e) => self.status = format!("Stop failed: {e}"),
                    }
                }
            }
            "steer" | "inject" => {
                let Some(id) = self.session.clone() else {
                    self.status = "Use /new or /use first".into();
                    return;
                };
                let text = if rest.is_empty() { arg.to_string() } else { format!("{arg} {rest}") };
                if text.trim().is_empty() {
                    self.status = "Usage: /steer <guidance inserted into the model context>".into();
                    return;
                }
                match self
                    .conn
                    .client
                    .call("chat.inject", json!({"session": id, "text": text}))
                {
                    Ok(_) => {
                        self.messages.push(("You (steer)".into(), text));
                        self.status = "Steer written to context; the current generation stops at a boundary and restarts".into();
                    }
                    Err(e) => self.status = format!("Failed to insert steer: {e}"),
                }
            }
            "compact" => {
                let Some(id) = self.session.clone() else {
                    self.status = "Use /new or /use first".into();
                    return;
                };
                match self.conn.client.call("context.compact", json!({"session": id})) {
                    Ok(v) => {
                        self.messages.push((
                            "System".into(),
                            format!(
                                "Context compacted: {} events → summary {} chars (covers through #{})",
                                v["compacted_events"], v["summary_chars"], v["covers_until_seq"]
                            ),
                        ));
                        self.status = "Compaction complete; original text remains in the session log".into();
                        self.refresh_ctx();
                    }
                    Err(e) => self.status = format!("Compaction failed: {e}"),
                }
            }
            "settings" | "config" => self.open_settings(),
            "plugins" => {
                self.refresh_plugins();
                self.status = "Plugin management: Space toggles, r refreshes, Esc back".into();
            }
            "clear" => self.messages.clear(),
            "help" => self.messages.push((
                "System".into(),
                "/new [id] [project] /workspace <project> /workspaces /use <id> /fork [id]\n/stop /steer <guidance> /compact /settings /plugins /clear /quit\n@<path> attaches a file; typing during a run = steer; /stop = emergency stop and save context".into(),
            )),
            _ => self.status = format!("Unknown command /{name}"),
        }
    }

    fn send(&mut self, text: &str) {
        let Some(id) = self.session.clone() else {
            self.status = "Use /new or /use to select a session".into();
            return;
        };
        self.focus = Focus::Chat;
        self.chat_scroll = 0;
        // Verify @ paths in the TUI first, then hand them to the kernel. The kernel rechecks
        // existence and copies files outside the install dir into the default workspace.
        let (clean, paths) = split_at_attachments(text);
        if !paths.is_empty() {
            for path in &paths {
                if !Path::new(path).is_file() {
                    self.messages
                        .push(("System".into(), format!("Attachment not found: {path}")));
                    self.status = format!("Attachment not found: {path}");
                    return;
                }
            }
        }
        let mut final_text = clean.trim().to_string();
        for path in paths {
            let source = std::fs::canonicalize(&path)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| path.clone());
            match self
                .conn
                .client
                .call("file.prepare", json!({"session": id, "path": source}))
            {
                Ok(v) => {
                    let resolved = v
                        .get("path")
                        .and_then(|x| x.as_str())
                        .unwrap_or(path.as_str());
                    if !final_text.is_empty() {
                        final_text.push_str("\n\n");
                    }
                    final_text.push_str(&format!("[Attachment] {resolved}"));
                }
                Err(e) => {
                    self.messages
                        .push(("System".into(), format!("Failed to attach file: {e}")));
                    self.status = e;
                    return;
                }
            }
        }
        if final_text.is_empty() {
            self.status = "Message and attachments are both empty".into();
            return;
        }
        let running = matches!(
            self.state.as_str(),
            "running" | "assembling" | "generating" | "tool_executing"
        );
        if running {
            self.messages
                .push(("You (steer)".into(), final_text.clone()));
            match self
                .conn
                .client
                .call("chat.inject", json!({"session": id, "text": final_text}))
            {
                Ok(_) => {
                    self.status =
                        "Steer inserted; the current generation continues after interruption".into()
                }
                Err(e) => {
                    self.messages
                        .push(("System".into(), format!("Failed to insert steer: {e}")));
                    self.status = e;
                }
            }
            return;
        }
        self.messages.push(("You".into(), final_text.clone()));
        match self
            .conn
            .client
            .call("chat.send", json!({"session": id, "text": final_text}))
        {
            Ok(_) => {
                self.state = "running".into();
                self.status = format!("Sending… ({id})");
            }
            Err(e) => {
                self.messages
                    .push(("System".into(), format!("Send failed: {e}")));
                self.status = e;
            }
        }
    }

    fn drain_events(&mut self) {
        while let Ok(v) = self.conn.events.try_recv() {
            if v.get("method").and_then(|m| m.as_str()) != Some("event") {
                if v.get("method").and_then(|m| m.as_str()) == Some("tool.confirm_request") {
                    let token = v
                        .pointer("/params/token")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                    let tool = v
                        .pointer("/params/tool")
                        .and_then(|x| x.as_str())
                        .unwrap_or("?")
                        .to_string();
                    let risk = v
                        .pointer("/params/risk")
                        .and_then(|x| x.as_str())
                        .unwrap_or("dangerous")
                        .to_string();
                    // 内核把 arguments 和 preview 一起送来；缺 preview 时退化成紧凑 JSON，
                    // 保证任何老内核下弹窗也至少能看到参数。
                    let preview = v
                        .pointer("/params/preview")
                        .and_then(|x| x.as_str())
                        .filter(|x| !x.trim().is_empty())
                        .map(|x| x.to_string())
                        .unwrap_or_else(|| {
                            v.pointer("/params/arguments")
                                .map(|a| {
                                    serde_json::to_string_pretty(a)
                                        .unwrap_or_else(|_| a.to_string())
                                })
                                .unwrap_or_else(|| "(no parameters provided by kernel)".to_string())
                        });
                    self.confirm = Some(ConfirmRequest {
                        token,
                        tool,
                        risk,
                        preview,
                    });
                }
                continue;
            }
            let kind = v
                .pointer("/params/kind")
                .and_then(|x| x.as_str())
                .unwrap_or("");
            let p = v.pointer("/params/payload").cloned().unwrap_or(Value::Null);
            let sess = p.get("session").and_then(|x| x.as_str()).unwrap_or("");
            if self.session.as_deref() != Some(sess) && sess != "_system" {
                continue;
            }
            match kind {
                "user.message" => {
                    if p.get("source").and_then(|x| x.as_str()) == Some("steer") {
                        let t = p.get("text").and_then(|x| x.as_str()).unwrap_or("");
                        self.messages.push(("You (steer)".into(), t.to_string()));
                    }
                }
                "model.message" => {
                    let t = p.get("text").and_then(|x| x.as_str()).unwrap_or("");
                    self.messages.push(("Assistant".into(), t.to_string()));
                }
                "tool.call" => {
                    let t = p.get("tool").and_then(|x| x.as_str()).unwrap_or("?");
                    self.messages.push(("Tool".into(), format!("Request {t}")));
                }
                "tool.result" => {
                    let t = p.get("name").and_then(|x| x.as_str()).unwrap_or("?");
                    let r = p.get("result").cloned().unwrap_or(Value::Null);
                    let brief = r.to_string().chars().take(200).collect::<String>();
                    self.messages
                        .push(("Tool".into(), format!("{t} → {brief}")));
                }
                "tool.error" => {
                    let t = p.get("name").and_then(|x| x.as_str()).unwrap_or("?");
                    self.messages.push((
                        "Tool".into(),
                        format!(
                            "{t} business error: {}",
                            p.get("error").and_then(|x| x.as_str()).unwrap_or("")
                        ),
                    ));
                }
                "tool.failed" => {
                    let t = p.get("name").and_then(|x| x.as_str()).unwrap_or("?");
                    self.messages.push((
                        "Tool".into(),
                        format!(
                            "{t} plugin failure: {}",
                            p.get("error").and_then(|x| x.as_str()).unwrap_or("")
                        ),
                    ));
                }
                "tool.degraded" => {
                    let t = p.get("name").and_then(|x| x.as_str()).unwrap_or("?");
                    let n = p.get("failures").and_then(|x| x.as_u64()).unwrap_or(3);
                    self.messages.push((
                        "System".into(),
                        format!("Tool circuit-breaker: {t} ({n} consecutive technical failures)"),
                    ));
                }
                "config.changed" => {
                    self.messages
                        .push(("System".into(), "Kernel configuration updated".into()));
                }
                "context.compacted" => {
                    let n = p
                        .get("compacted_events")
                        .and_then(|x| x.as_u64())
                        .unwrap_or(0);
                    let chars = p.get("after_chars").and_then(|x| x.as_u64()).unwrap_or(0);
                    self.messages.push((
                        "System".into(),
                        format!("Context compacted: {n} old events → {chars}-char summary (original text remains in the log)"),
                    ));
                    self.refresh_ctx();
                }
                "permission.denied" => {
                    self.messages.push((
                        "System".into(),
                        format!(
                            "Denied: {}",
                            p.get("reason").and_then(|x| x.as_str()).unwrap_or("")
                        ),
                    ));
                }
                "plugin.degraded" => {
                    self.messages.push((
                        "System".into(),
                        "Plugin degraded (3 consecutive failures)".into(),
                    ));
                }
                "subscriber.lagged" => {
                    self.messages.push((
                        "System".into(),
                        "Event subscription lagged; old events dropped".into(),
                    ));
                }
                "chat.finished" => {
                    let s = p.get("status").and_then(|x| x.as_str()).unwrap_or("done");
                    self.messages
                        .push(("System".into(), format!("— Turn complete ({s})")));
                    self.state = if s == "cancelled" || s == "stopped" {
                        "cancelled".into()
                    } else {
                        "idle".into()
                    };
                    self.status = format!("Done ({s})");
                    self.refresh_ctx();
                    self.refresh_sessions();
                }
                "memory.recalled" => {
                    let n = p
                        .get("items")
                        .and_then(|x| x.as_array())
                        .map(|a| a.len())
                        .unwrap_or(0);
                    if n > 0 {
                        self.messages
                            .push(("System".into(), format!("Recalled {n} relevant memories")));
                    }
                }
                _ => {}
            }
        }
    }
}

fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| UnicodeWidthChar::width(c).unwrap_or(0))
        .sum()
}

fn event_message(kind: &str, p: &Value) -> Option<(String, String)> {
    let text = match kind {
        "user.message" => ("You".to_string(), p.get("text")?.as_str()?.to_string()),
        "model.message" => {
            let t = p.get("text").and_then(|x| x.as_str()).unwrap_or("");
            if t.is_empty() {
                return None;
            }
            ("Assistant".to_string(), t.to_string())
        }
        "tool.requested" => (
            "Tool".to_string(),
            format!(
                "Request {}",
                p.get("tool").and_then(|x| x.as_str()).unwrap_or("?")
            ),
        ),
        "tool.result" => {
            let name = p.get("name").and_then(|x| x.as_str()).unwrap_or("?");
            let brief: String = p
                .get("result")
                .cloned()
                .unwrap_or(Value::Null)
                .to_string()
                .chars()
                .take(200)
                .collect();
            ("Tool".to_string(), format!("{name} → {brief}"))
        }
        "tool.error" => (
            "Tool".to_string(),
            format!(
                "{} business error: {}",
                p.get("name").and_then(|x| x.as_str()).unwrap_or("?"),
                p.get("error").and_then(|x| x.as_str()).unwrap_or("")
            ),
        ),
        "tool.failed" => (
            "Tool".to_string(),
            format!(
                "{} plugin failure: {}",
                p.get("name").and_then(|x| x.as_str()).unwrap_or("?"),
                p.get("error").and_then(|x| x.as_str()).unwrap_or("")
            ),
        ),
        "permission.denied" => (
            "System".to_string(),
            format!(
                "Denied: {}",
                p.get("reason").and_then(|x| x.as_str()).unwrap_or("")
            ),
        ),
        "chat.finished" => (
            "System".to_string(),
            format!(
                "— Turn complete ({})",
                p.get("status").and_then(|x| x.as_str()).unwrap_or("done")
            ),
        ),
        _ => return None,
    };
    Some(text)
}

fn md_inline(input: &str, color: Color) -> Vec<(String, Style)> {
    let chars: Vec<char> = input.chars().collect();
    let mut out: Vec<(String, Style)> = Vec::new();
    let mut buf = String::new();
    let base = Style::default().fg(color);
    let mut i = 0usize;
    let flush = |buf: &mut String, out: &mut Vec<(String, Style)>| {
        if !buf.is_empty() {
            out.push((std::mem::take(buf), base));
        }
    };
    while i < chars.len() {
        if chars[i] == '`' {
            if let Some(end) = chars[i + 1..].iter().position(|c| *c == '`') {
                flush(&mut buf, &mut out);
                let text: String = chars[i + 1..i + 1 + end].iter().collect();
                out.push((text, Style::default().fg(Color::Yellow)));
                i += end + 2;
                continue;
            }
        }
        if chars[i] == '*' && i + 1 < chars.len() && chars[i + 1] == '*' {
            if let Some(end) = chars[i + 2..]
                .windows(2)
                .position(|w| w[0] == '*' && w[1] == '*')
            {
                flush(&mut buf, &mut out);
                let text: String = chars[i + 2..i + 2 + end].iter().collect();
                out.push((
                    text,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ));
                i += end + 4;
                continue;
            }
        }
        if (chars[i] == '*' || chars[i] == '_')
            && (i == 0 || chars[i - 1] != chars[i])
            && i + 1 < chars.len()
            && chars[i + 1] != chars[i]
        {
            let marker = chars[i];
            if let Some(end) = chars[i + 1..].iter().position(|c| *c == marker) {
                if end > 0 {
                    flush(&mut buf, &mut out);
                    let text: String = chars[i + 1..i + 1 + end].iter().collect();
                    out.push((
                        text,
                        Style::default().fg(color).add_modifier(Modifier::ITALIC),
                    ));
                    i += end + 2;
                    continue;
                }
            }
        }
        if chars[i] == '[' {
            if let Some(close) = chars[i + 1..].iter().position(|c| *c == ']') {
                let label: String = chars[i + 1..i + 1 + close].iter().collect();
                let after = i + close + 2;
                if after < chars.len() && chars[after] == '(' {
                    if let Some(paren) = chars[after + 1..].iter().position(|c| *c == ')') {
                        let url: String = chars[after + 1..after + 1 + paren].iter().collect();
                        flush(&mut buf, &mut out);
                        if url.starts_with("http://") || url.starts_with("https://") {
                            out.push((label, Style::default().fg(Color::Blue)));
                            out.push((format!(" ({url})"), Style::default().fg(Color::DarkGray)));
                            i = after + paren + 2;
                            continue;
                        }
                    }
                }
            }
        }
        buf.push(chars[i]);
        i += 1;
    }
    flush(&mut buf, &mut out);
    out
}

fn md_line(
    out: &mut Vec<Line<'static>>,
    prefix: &str,
    cont: &str,
    segs: &[(String, Style)],
    width: usize,
    base: Style,
) {
    let width = width.max(1);
    let mut chars: Vec<(char, Style)> = prefix.chars().map(|c| (c, base)).collect();
    let mut w = prefix
        .chars()
        .map(|c| UnicodeWidthChar::width(c).unwrap_or(0))
        .sum::<usize>();
    let mut span_start = 0usize;
    let flush = |chars: &mut Vec<(char, Style)>, out: &mut Vec<Line<'static>>| {
        if chars.is_empty() {
            out.push(Line::from(""));
            return;
        }
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut cur = String::new();
        let mut cur_style = chars[0].1;
        for (ch, style) in chars.iter() {
            if *style != cur_style && !cur.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut cur), cur_style));
                cur_style = *style;
            }
            cur.push(*ch);
        }
        if !cur.is_empty() {
            spans.push(Span::styled(cur, cur_style));
        }
        out.push(Line::from(spans));
        chars.clear();
    };
    for (ch, style) in segs
        .iter()
        .flat_map(|(s, st)| s.chars().map(move |c| (c, *st)))
    {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if !chars.is_empty() && w + cw > width {
            flush(&mut chars, out);
            chars = cont
                .chars()
                .map(|c| (c, Style::default().fg(Color::DarkGray)))
                .collect();
            w = cont
                .chars()
                .map(|c| UnicodeWidthChar::width(c).unwrap_or(0))
                .sum::<usize>();
        }
        chars.push((ch, style));
        w += cw;
        span_start += 1;
    }
    let _ = span_start;
    flush(&mut chars, out);
}

fn markdown_lines(text: &str, width: usize, color: Color) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let mut in_code = false;
    for raw in text.replace("\t", "    ").lines() {
        let line = raw.trim_end();
        if line.trim_start().starts_with("```") {
            if in_code {
                in_code = false;
                out.push(Line::from(Span::styled(
                    "╰─",
                    Style::default().fg(Color::DarkGray),
                )));
            } else {
                in_code = true;
                let lang = line.trim().trim_start_matches("```").trim();
                out.push(Line::from(Span::styled(
                    if lang.is_empty() {
                        "╭─ code".to_string()
                    } else {
                        format!("╭─ {lang}")
                    },
                    Style::default().fg(Color::DarkGray),
                )));
            }
            continue;
        }
        if in_code {
            md_line(
                &mut out,
                "│ ",
                "│ ",
                &[(line.to_string(), Style::default().fg(Color::Yellow))],
                width,
                Style::default().fg(Color::Yellow),
            );
            continue;
        }
        if line.is_empty() {
            out.push(Line::from(""));
            continue;
        }
        if line.chars().all(|c| c == '-' || c == '*' || c == '_') {
            out.push(Line::from(Span::styled(
                "─".repeat(width.min(120)),
                Style::default().fg(Color::DarkGray),
            )));
            continue;
        }
        let heading = line.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&heading) && line.chars().nth(heading) == Some(' ') {
            let content = line[heading + 1..].to_string();
            md_line(
                &mut out,
                "▌ ",
                "  ",
                &md_inline(&content, Color::White),
                width,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            );
            continue;
        }
        let trimmed = line.trim_start();
        let ordered = {
            let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
            let n = digits.len();
            if n <= 2 && n < trimmed.len() && trimmed[n..].starts_with(". ") {
                Some((digits, trimmed[n + 2..].to_string()))
            } else {
                None
            }
        };
        if let Some((num, rest)) = ordered {
            let prefix = format!("{num}. ");
            let cont = " ".repeat(prefix.chars().count());
            md_line(
                &mut out,
                &prefix,
                &cont,
                &md_inline(&rest, color),
                width,
                Style::default().fg(color),
            );
            continue;
        }
        if let Some(rest) = line
            .trim_start()
            .strip_prefix("- ")
            .or_else(|| line.trim_start().strip_prefix("* "))
            .or_else(|| line.trim_start().strip_prefix("+ "))
        {
            md_line(
                &mut out,
                "• ",
                "  ",
                &md_inline(rest, color),
                width,
                Style::default().fg(color),
            );
            continue;
        }
        if let Some(rest) = line.trim_start().strip_prefix("> ") {
            md_line(
                &mut out,
                "│ ",
                "│ ",
                &md_inline(rest, Color::DarkGray),
                width,
                Style::default().fg(Color::DarkGray),
            );
            continue;
        }
        md_line(
            &mut out,
            "",
            "",
            &md_inline(line, color),
            width,
            Style::default().fg(color),
        );
    }
    if in_code {
        out.push(Line::from(Span::styled(
            "╰─",
            Style::default().fg(Color::DarkGray),
        )));
    }
    out
}

fn wrap_display(prefix: &str, text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for (i, para) in text.split('\n').enumerate() {
        let pfx = if i == 0 { prefix } else { "   " };
        let mut cur = String::new();
        let mut w = 0usize;
        for ch in pfx.chars().chain(para.chars()) {
            let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
            if w + cw > width && !cur.is_empty() {
                out.push(cur);
                cur = String::new();
                w = 0;
            }
            cur.push(ch);
            w += cw;
        }
        out.push(cur);
    }
    out
}

fn clamp_chat_scroll(scroll: usize, max_scroll: usize) -> usize {
    scroll.min(max_scroll)
}

fn draw(f: &mut Frame, app: &mut App) {
    if app.plugins.is_some() {
        draw_plugins(f, app);
        if let Some(c) = &app.confirm {
            draw_confirm(f, c, f.size());
        }
        return;
    }
    if app.settings.is_some() {
        draw_settings(f, app);
        if let Some(c) = &app.confirm {
            draw_confirm(f, c, f.size());
        }
        return;
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(6),
            Constraint::Length(3),
        ])
        .split(f.size());

    let mode = match app.focus {
        Focus::Sessions => "Sessions",
        Focus::Chat => "Conversation",
    };
    let head = format!(
        " FeatherAgent v{}  {}  Session: {}  Status: {}  |  {}",
        env!("CARGO_PKG_VERSION"),
        mode,
        app.session.as_deref().unwrap_or("-"),
        app.state,
        app.status
    );
    f.render_widget(
        Paragraph::new(head).style(Style::default().fg(Color::Black).bg(Color::Cyan)),
        chunks[0],
    );

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(18),
            Constraint::Percentage(52),
            Constraint::Percentage(30),
        ])
        .split(chunks[1]);

    // 左：会话
    let items: Vec<ListItem> = app
        .sessions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let id = s["id"].as_str().unwrap_or("?");
            let n = s["records"].as_u64().unwrap_or(0);
            let live = s["live"]["state"].as_str().unwrap_or("");
            let ws = s["workspace"].as_str().unwrap_or("default");
            let text = format!("{id}\n{ws} · {n} events {live}");
            let style = if i == app.selected && app.focus == Focus::Sessions {
                Style::default().bg(Color::DarkGray)
            } else if i == app.selected {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default()
            };
            ListItem::new(text).style(style)
        })
        .collect();
    let mut list_state = ListState::default().with_selected(Some(app.selected));
    f.render_stateful_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title(
            if app.focus == Focus::Sessions {
                "Sessions ↑↓ / Enter"
            } else {
                "Sessions (Esc back)"
            },
        )),
        cols[0],
        &mut list_state,
    );

    // 中：对话流。先按显示宽度手工折行，滚动量就是可视行数，底部不会因
    // Paragraph 的软换行估算偏差而滚成空白。
    let width = cols[1].width.saturating_sub(2).max(1) as usize;
    let mut lines: Vec<Line> = Vec::new();
    for (role, text) in &app.messages {
        let color = match role.as_str() {
            "You" => Color::Green,
            "Assistant" => Color::Cyan,
            "Tool" => Color::Yellow,
            "System" => Color::DarkGray,
            _ => Color::White,
        };
        if role == "Assistant" {
            lines.push(Line::from(Span::styled(
                "Assistant:".to_string(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )));
            lines.extend(markdown_lines(text, width, color));
        } else {
            let prefix = format!("{role}: ");
            for chunk in wrap_display(&prefix, text, width) {
                lines.push(Line::from(Span::styled(
                    chunk,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )));
            }
        }
    }
    let h = cols[1].height.saturating_sub(2) as usize;
    let max_scroll = lines.len().saturating_sub(h);
    // 在顶部继续按 Up 时把状态夹紧，否则向下翻要先抵消掉多余的虚拟增量。
    app.chat_scroll = clamp_chat_scroll(app.chat_scroll, max_scroll);
    let scroll = max_scroll.saturating_sub(app.chat_scroll);
    let visible: Vec<Line> = lines.into_iter().skip(scroll).take(h).collect();
    f.render_widget(
        Paragraph::new(visible).block(Block::default().borders(Borders::ALL).title(
            if app.focus == Focus::Chat {
                "Conversation ↑↓ scroll · Esc back to list"
            } else {
                "Conversation (Enter to open session)"
            },
        )),
        cols[1],
    );

    // 右：上下文面板
    let mut ctx_lines = Vec::new();
    if app.ctx.is_null() {
        ctx_lines.push(Line::from("(no snapshot yet; send a message first)"));
    } else {
        let st = &app.ctx["stats"];
        let window = st["window_tokens"].as_u64().unwrap_or(8192) as usize;
        let total_tokens = st["total_tokens"].as_u64().unwrap_or(0) as usize;
        let pct = total_tokens
            .saturating_mul(100)
            .checked_div(window)
            .unwrap_or(0);
        ctx_lines.push(Line::from(format!(
            "Budget {total_tokens}/{window} ({pct}%)"
        )));
        ctx_lines.push(Line::from(format!(
            "Layers L1 {} items / L2 {} cards / L3 {} tok / L4 {} items",
            st["layers"]["l1_window_messages"].as_u64().unwrap_or(0),
            st["layers"]["l2_summary_cards"].as_u64().unwrap_or(0),
            st["layers"]["l3_task_state_tokens"].as_u64().unwrap_or(0),
            st["layers"]["l4_recalled_items"].as_u64().unwrap_or(0),
        )));
        ctx_lines.push(Line::from(format!(
            "system {} · tools {} · skills {} · history {} · recall {}",
            st["system_tokens"].as_u64().unwrap_or(0),
            st["tools_tokens"].as_u64().unwrap_or(0),
            st["skills_tokens"].as_u64().unwrap_or(0),
            st["history_tokens"].as_u64().unwrap_or(0),
            st["recall_tokens"].as_u64().unwrap_or(0),
        )));
        ctx_lines.push(Line::from(format!(
            "Model {} · events {}",
            app.ctx["model"].as_str().unwrap_or("-"),
            app.ctx["turns"].as_u64().unwrap_or(0),
        )));
        let goal = app.ctx["task_state"]["goal"].as_str().unwrap_or("");
        if !goal.is_empty() {
            ctx_lines.push(Line::from(format!("Goal: {goal}")));
        }
        let cards = app.ctx["cards"].as_u64().unwrap_or(0);
        ctx_lines.push(Line::from(format!("Summary cards {cards}")));
    }
    if let Some(c) = &app.confirm {
        ctx_lines.push(Line::from(""));
        ctx_lines.push(Line::from(Span::styled(
            format!("⚠ Pending [{}] {}", c.risk, c.tool),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
        ctx_lines.push(Line::from("y allow / n deny (full command in dialog)"));
    }
    f.render_widget(
        Paragraph::new(ctx_lines)
            .block(Block::default().borders(Borders::ALL).title("Context"))
            .wrap(Wrap { trim: true }),
        cols[2],
    );

    // 底：输入。按显示宽度（CJK 占 2 列）计算光标，并在超出时横向滚动。
    let area = chunks[2];
    let chars: Vec<char> = app.input.chars().collect();
    let cursor = app.input_cursor.min(chars.len());
    let avail = area.width.saturating_sub(5) as usize;
    let mut start = 0usize;
    while start < cursor && display_width(&chars[start..cursor].iter().collect::<String>()) > avail
    {
        start += 1;
    }
    let tail: String = chars[start..].iter().collect();
    let title = if app.focus == Focus::Chat {
        "Input (Enter sends, ←/→ moves cursor, /help commands)"
    } else {
        "Input (sessions: Enter open, ↑↓ select, Esc quit)"
    };
    f.render_widget(
        Paragraph::new(format!("› {tail}"))
            .block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );
    if app.focus == Focus::Chat {
        let before: String = chars[start..cursor].iter().collect();
        let x = area.x + 1 + 2 + display_width(&before) as u16;
        let y = area.y + 1;
        f.set_cursor(x.min(area.right().saturating_sub(1)), y);
    }
    if let Some(c) = &app.confirm {
        draw_confirm(f, c, f.size());
    }
}

/// 权限申请弹窗：把工具名、风险等级和完整命令摊开，用户不必猜“这条 y 到底放行什么”。
fn draw_confirm(f: &mut Frame, c: &ConfirmRequest, area: Rect) {
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            "⚠ Dangerous tool needs confirmation",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("Tool: {}    Risk: {}", c.tool, c.risk)),
        Line::from(""),
        Line::from(Span::styled(
            "About to run:",
            Style::default().add_modifier(Modifier::BOLD),
        )),
    ];
    for l in c.preview.lines().take(400) {
        lines.push(Line::from(l.to_string()));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "y allow / n or Esc deny",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));
    let want_h = lines.len() as u16 + 2;
    let w = area.width.saturating_sub(8).clamp(40, 110);
    let h = want_h.min(area.height.saturating_sub(4)).max(6);
    let rect = Rect::new(
        area.x + area.width.saturating_sub(w) / 2,
        area.y + area.height.saturating_sub(h) / 2,
        w,
        h,
    );
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Permission request"),
        ),
        rect,
    );
}

fn draw_settings(f: &mut Frame, app: &App) {
    let Some(s) = &app.settings else { return };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(4),
        ])
        .split(f.size());

    let head = Paragraph::new(Line::from(vec![
        Span::styled(
            format!(" FeatherAgent {} Settings ", featheragent::VERSION),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(if s.nav_focus {
            "  ↑↓ sidebar · Enter open · Tab to form · Esc close"
        } else if s.detail {
            "  Space/Enter toggle · r refresh · Esc back to sidebar"
        } else {
            "  Tab/↑↓ switch field · ←/→ select provider/model/adjust value · Esc back to sidebar"
        }),
    ]));
    f.render_widget(head, chunks[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(27), Constraint::Min(30)])
        .split(chunks[1]);

    // 左侧栏：常规设置 + 插件动态条目（与 WebUI 同构）。
    let mut nav_lines: Vec<Line> = Vec::new();
    let general_selected = s.nav_idx == 0;
    let mut g = Line::from(format!(
        "{} ⚙ General Settings",
        if general_selected && s.nav_focus {
            "▶"
        } else {
            " "
        }
    ));
    if general_selected && s.nav_focus {
        g = g.style(
            Style::default()
                .bg(Color::DarkGray)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        );
    }
    nav_lines.push(g);
    nav_lines.push(Line::from(Span::styled(
        "Plugin page",
        Style::default().fg(Color::DarkGray),
    )));
    for (i, item) in s.nav.iter().enumerate() {
        let name = item.get("name").and_then(|x| x.as_str()).unwrap_or("?");
        let enabled = item
            .get("enabled")
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        let live = item
            .get("live_state")
            .and_then(|x| x.as_str())
            .unwrap_or("stopped");
        let selected = s.nav_idx == i + 1;
        let marker = if selected && s.nav_focus { "▶" } else { " " };
        let mut l = Line::from(format!(
            "{marker} {} {name}",
            if enabled { "●" } else { "○" }
        ));
        if selected && s.nav_focus {
            l = l.style(
                Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            );
        } else if !enabled {
            l = l.style(Style::default().fg(Color::DarkGray));
        }
        nav_lines.push(l);
        if selected && s.nav_focus {
            nav_lines.push(Line::from(Span::styled(
                format!("    live={live}"),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
    nav_lines.push(Line::from(""));
    nav_lines.push(Line::from(Span::styled(
        "Plugins come from the kernel registry",
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(
        Paragraph::new(nav_lines)
            .block(Block::default().borders(Borders::ALL).title("Sidebar"))
            .wrap(Wrap { trim: false }),
        body[0],
    );

    if s.nav_idx > 0 {
        let item = s.nav.get(s.nav_idx - 1).cloned().unwrap_or(Value::Null);
        if let Some(p) = &s.plugin {
            let enabled = item
                .get("enabled")
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            let category = item
                .get("category")
                .and_then(|x| x.as_str())
                .unwrap_or("plugin");
            draw_plugin_form(f, p, enabled, category, body[1]);
        } else {
            let name = item.get("name").and_then(|x| x.as_str()).unwrap_or("?");
            let enabled = item
                .get("enabled")
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            let live = item
                .get("live_state")
                .and_then(|x| x.as_str())
                .unwrap_or("stopped");
            let category = item
                .get("category")
                .and_then(|x| x.as_str())
                .unwrap_or("plugin");
            let program = item.get("program").and_then(|x| x.as_str()).unwrap_or("-");
            let hash = item.get("blake3").and_then(|x| x.as_str()).unwrap_or("-");
            let version = item.get("version").and_then(|x| x.as_str()).unwrap_or("-");
            let caps = item.get("capabilities").cloned().unwrap_or(Value::Null);
            let perms = item.get("permissions").cloned().unwrap_or(Value::Null);
            let lines = vec![
                Line::from(vec![
                    Span::styled(name, Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(format!("  [{category}]")),
                ]),
                Line::from(""),
                Line::from(format!(
                    "Status      {}",
                    if enabled { "Enabled" } else { "Disabled" }
                )),
                Line::from(format!("live      {live}")),
                Line::from(format!("Version      {version}")),
                Line::from(format!("capabilities  {}", caps)),
                Line::from(format!("permissions   {}", perms)),
                Line::from(format!("program   {program}")),
                Line::from(format!("blake3    {hash}")),
                Line::from(""),
                Line::from(if s.detail {
                    "Space/Enter toggle · r refresh · Esc back to sidebar"
                } else {
                    "Enter opens details"
                }),
            ];
            f.render_widget(
                Paragraph::new(lines)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Plugin details"),
                    )
                    .wrap(Wrap { trim: false }),
                body[1],
            );
        }
    } else {
        let values = [
            ("Provider", s.provider.clone()),
            ("Base URL", s.base_url.clone()),
            ("Model", s.model.clone()),
            (
                "API Key",
                if s.api_key.is_empty() {
                    "(not set; local services usually leave empty)".into()
                } else if s.field == 3 && !s.nav_focus {
                    format!(
                        "{} characters entered (Ctrl+U clears)",
                        s.api_key.chars().count()
                    )
                } else {
                    "••••••••".into()
                },
            ),
            ("Temperature", format!("{}（0.00–2.00）", s.temperature)),
            ("Context window", format!("{} tokens", s.context_window)),
            ("Max turns", s.max_turns.clone()),
            (
                "Workspace root",
                if s.workspace_root.is_empty() {
                    "(install root/workspace: project container)".into()
                } else {
                    s.workspace_root.clone()
                },
            ),
            (
                "Auto allow safe",
                if s.auto_allow_safe {
                    "[x] On".into()
                } else {
                    "[ ] Off".into()
                },
            ),
            ("Save and apply", "Press Enter / Ctrl+S".into()),
            ("Cancel/Back", "Esc back to sidebar".into()),
        ];
        let mut lines = Vec::new();
        for (i, (label, val)) in values.iter().enumerate() {
            let marker = if i == s.field && !s.nav_focus {
                "▶ "
            } else {
                "  "
            };
            let text = format!("{marker}{label:<16} {val}");
            let mut line = Line::from(text);
            if i == s.field && !s.nav_focus {
                line = line.style(
                    Style::default()
                        .bg(Color::DarkGray)
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                );
            }
            lines.push(line);
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Each plugin's settings are on its own page on the left (declared by plugin, relayed by kernel)",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(Span::styled(
            &s.models_hint,
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(Span::styled(
            &s.search_hint,
            Style::default().fg(Color::DarkGray),
        )));
        for m in s.models.iter().take(12) {
            lines.push(Line::from(format!("  · {m}")));
        }
        // 11 个字段在 80x24 终端放不下；只滚动表单，保证当前字段可见。
        let viewport = body[1].height.saturating_sub(2) as usize;
        let offset = if viewport > 0 && s.field >= viewport {
            (s.field + 1 - viewport) as u16
        } else {
            0
        };
        f.render_widget(
            Paragraph::new(lines)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("General (submitted via config.set on save)"),
                )
                .wrap(Wrap { trim: false })
                .scroll((offset, 0)),
            body[1],
        );
    }

    let plugin_status = s.plugin.as_ref().map(|p| p.status.clone());
    let status = match (&plugin_status, s.status.is_empty()) {
        (Some(p), _) => p.as_str(),
        (None, true) => "Settings page opened",
        (None, false) => s.status.as_str(),
    };
    f.render_widget(
        Paragraph::new(status)
            .style(Style::default().fg(Color::Yellow))
            .block(Block::default().borders(Borders::ALL).title("Status")),
        chunks[2],
    );
}

/// 插件设置页：完全由内核转述的 `plugin.settings` 声明驱动，TUI 不认识具体插件。
fn draw_plugin_form(f: &mut Frame, p: &PluginFormState, enabled: bool, category: &str, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(
            p.name.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "  [{category}]  {}",
            if enabled { "Enabled" } else { "Disabled" }
        )),
        if p.dirty {
            Span::styled("  Unsaved", Style::default().fg(Color::Yellow))
        } else {
            Span::raw("")
        },
    ]));
    if !p.description.trim().is_empty() {
        lines.push(Line::from(Span::styled(
            p.description.clone(),
            Style::default().fg(Color::DarkGray),
        )));
    }
    lines.push(Line::from(""));

    // 字段 0：启停开关；1..=n：声明字段；之后保存/取消。
    let rows = p.len();
    for i in 0..rows {
        let marker = if i == p.field { "▶ " } else { "  " };
        let text = if i == 0 {
            format!(
                "{marker}{:<16} {}",
                "Enable plugin",
                if p.enabled { "[x] On" } else { "[ ] Off" }
            )
        } else if i == p.save_idx() {
            format!("{marker}{:<16} Enter / Ctrl+S", "Save and apply")
        } else if i == p.cancel_idx() {
            format!("{marker}{:<16} Esc", "Cancel/Back")
        } else {
            let f = &p.fields[i - 1];
            let label = f.get("label").and_then(|x| x.as_str()).unwrap_or("?");
            let kind = f.get("type").and_then(|x| x.as_str()).unwrap_or("string");
            let shown = match kind {
                "bool" => {
                    if p.values[i - 1].as_bool().unwrap_or(false) {
                        "[x] On".to_string()
                    } else {
                        "[ ] Off".to_string()
                    }
                }
                "enum" => p.values[i - 1].as_str().unwrap_or("").to_string(),
                _ => {
                    let t = &p.text[i - 1];
                    if t.trim().is_empty() {
                        f.get("placeholder")
                            .and_then(|x| x.as_str())
                            .unwrap_or("(empty)")
                            .to_string()
                    } else {
                        t.clone()
                    }
                }
            };
            let hint = match kind {
                "int" | "number" => {
                    let mut h = String::new();
                    if let Some(m) = f.get("min").and_then(|x| x.as_f64()) {
                        h.push_str(&format!("{m}–"));
                    }
                    if let Some(m) = f.get("max").and_then(|x| x.as_f64()) {
                        h.push_str(&m.to_string());
                    }
                    if h.is_empty() {
                        String::new()
                    } else {
                        format!("（{h}）")
                    }
                }
                _ => String::new(),
            };
            let hint = if f
                .get("options_source")
                .and_then(|x| x.as_str())
                .map(|x| x == "models")
                .unwrap_or(false)
                && matches!(kind, "string")
                && !p.models.is_empty()
            {
                format!("{hint} (←/→ select)")
            } else if f
                .get("options_source")
                .and_then(|x| x.as_str())
                .map(|x| x == "models")
                .unwrap_or(false)
                && matches!(kind, "string")
            {
                format!("{hint} (list unavailable; type manually)")
            } else {
                hint
            };
            format!("{marker}{label:<16} {shown}{hint}")
        };
        let mut line = Line::from(text);
        if i == p.field {
            line = line.style(
                Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            );
        }
        lines.push(line);
    }

    // 当前字段的说明放在列表下方，避免长描述撑乱表格。
    if p.field >= 1 && p.field <= p.fields.len() {
        let f = &p.fields[p.field - 1];
        if let Some(d) = f.get("description").and_then(|x| x.as_str()) {
            if !d.trim().is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    d.to_string(),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
    }
    let viewport = area.height.saturating_sub(2) as usize;
    let offset = if viewport > 0 && p.field >= viewport {
        (p.field + 1 - viewport) as u16
    } else {
        0
    };
    f.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!("{} (declared by plugin)", p.title)),
            )
            .wrap(Wrap { trim: false })
            .scroll((offset, 0)),
        area,
    );
}

fn draw_plugins(f: &mut Frame, app: &App) {
    let Some(p) = &app.plugins else { return };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(4),
        ])
        .split(f.size());
    let head = Paragraph::new(Line::from(vec![
        Span::styled(
            format!(
                " FeatherAgent v{} Plugin Management ",
                env!("CARGO_PKG_VERSION")
            ),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  ↑↓ select · Space/Enter toggle · r refresh · Esc back"),
    ]));
    f.render_widget(head, chunks[0]);

    let mut lines = Vec::new();
    if p.items.is_empty() {
        lines.push(Line::from(
            "(no registered plugins; install by path from the WebUI Plugins page)",
        ));
    }
    for (i, item) in p.items.iter().enumerate() {
        let name = item.get("name").and_then(|x| x.as_str()).unwrap_or("?");
        let cat = item.get("category").and_then(|x| x.as_str()).unwrap_or("");
        let enabled = item
            .get("enabled")
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        let state = item
            .get("live_state")
            .and_then(|x| x.as_str())
            .unwrap_or("stopped");
        let mark = if enabled { "[x]" } else { "[ ]" };
        let marker = if i == p.selected { "▶ " } else { "  " };
        let line = format!("{marker}{mark} {name:<22} {cat:<10} live={state}");
        let mut l = Line::from(line);
        if i == p.selected {
            l = l.style(
                Style::default()
                    .bg(Color::DarkGray)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            );
        }
        lines.push(l);
    }
    f.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Plugins (toggle syncs to the running process)"),
            )
            .wrap(Wrap { trim: false }),
        chunks[1],
    );
    f.render_widget(
        Paragraph::new(p.status.as_str())
            .style(Style::default().fg(Color::Yellow))
            .block(Block::default().borders(Borders::ALL).title("Status")),
        chunks[2],
    );
}

fn split_at_attachments(text: &str) -> (String, Vec<String>) {
    let mut clean = String::new();
    let mut paths = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '@' && (i == 0 || chars[i - 1].is_whitespace()) {
            let mut j = i + 1;
            let quoted = j < chars.len() && (chars[j] == '"' || chars[j] == '\'');
            if quoted {
                let quote = chars[j];
                j += 1;
                let start = j;
                while j < chars.len() && chars[j] != quote {
                    j += 1;
                }
                if j > start {
                    paths.push(chars[start..j].iter().collect::<String>());
                    i = (j + 1).min(chars.len());
                    continue;
                }
            } else {
                let start = j;
                while j < chars.len() && !chars[j].is_whitespace() {
                    j += 1;
                }
                if j > start {
                    paths.push(chars[start..j].iter().collect::<String>());
                    i = j;
                    continue;
                }
            }
        }
        clean.push(chars[i]);
        i += 1;
    }
    (clean, paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_scroll_is_clamped_after_overscrolling_at_top() {
        assert_eq!(clamp_chat_scroll(usize::MAX, 8), 8);
        assert_eq!(clamp_chat_scroll(8, 8), 8);
        assert_eq!(clamp_chat_scroll(8, 8).saturating_sub(1), 7);
        assert_eq!(clamp_chat_scroll(0, 8).saturating_sub(1), 0);
    }
}
