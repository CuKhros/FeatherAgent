//! `featheragent-tui`：三栏终端前端（§7.3）。
//!
//! 左：会话列表；中：对话流；右：上下文面板（预算分层 / 任务态 / 模型）；
//! 底：输入行。危险工具确认用 y/n 应答。所有数据走 UDS 契约，不读内核内存。

use std::io::{stdout, Stdout};
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
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
    field: usize,
    status: String,
    dirty: bool,
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
        cur = entry.as_object_mut().expect("刚建的对象");
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
    confirm: Option<(String, String, String)>, // token, tool, risk
    settings: Option<SettingsState>,
    plugins: Option<PluginsState>,
    quit: bool,
}

fn main() {
    let args = Args::parse(std::env::args().skip(1));
    if args.has("--help") || args.positional.iter().any(|p| p == "-h") {
        println!("FeatherAgent TUI {}", env!("CARGO_PKG_VERSION"));
        println!("用法: featheragent-tui [--root DIR] [--socket PATH]");
        return;
    }
    if args.has("version") {
        println!("featheragent-tui {}", env!("CARGO_PKG_VERSION"));
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
            eprintln!("无法连接内核 {socket:?}: {e}\n先运行 `featheragent serve`");
            std::process::exit(1);
        }
    };
    if let Err(e) = conn
        .client
        .call_with_timeout("initialize", json!({}), Duration::from_secs(3))
    {
        eprintln!("初始化失败（3s 超时/内核未就绪）: {e}");
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
        status: format!("已连接 {}；↑↓ 选会话，Enter 进入", socket.display()),
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
            eprintln!("TUI 初始化失败: {e}");
            std::process::exit(1);
        }
    };
    let res = run(&mut terminal, &mut app);
    restore();
    if let Err(e) = res {
        eprintln!("TUI 退出: {e}");
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
                    "系统".into(),
                    format!("已恢复会话 {id}｜项目 {ws}｜{} 条事件", v["total"]),
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
                self.status = format!("会话 {id}：↑↓ 翻阅对话，Esc 返回列表，/help 查看命令");
                self.refresh_ctx();
            }
            Err(e) => self.status = format!("恢复失败: {e}"),
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
                self.messages.push(("系统".into(), format!("新会话 {id}")));
                self.focus = Focus::Chat;
                self.chat_scroll = 0;
                self.input.clear();
                self.input_cursor = 0;
                self.status = format!("会话 {id}：↑↓ 翻阅对话，Esc 返回列表");
                self.refresh_sessions();
                self.refresh_ctx();
            }
            Err(e) => self.status = format!("创建失败: {e}"),
        }
    }

    fn refresh_plugins(&mut self) {
        let v = match self.conn.client.call("plugin.list", json!({})) {
            Ok(v) => v,
            Err(e) => {
                if let Some(p) = &mut self.plugins {
                    p.status = format!("读取插件失败: {e}");
                } else {
                    self.status = format!("读取插件失败: {e}");
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
        let status = format!("{} 个插件；Space 启停，r 刷新，Esc 返回", items.len());
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
                        "{} → {}（已同步运行进程）",
                        name,
                        if enabled { "停用" } else { "启用" }
                    );
                }
            }
            Err(e) => {
                if let Some(p) = &mut self.plugins {
                    p.status = format!("启停失败: {e}");
                }
            }
        }
    }

    fn on_plugins_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => {
                self.plugins = None;
                self.status = "已关闭插件管理".into();
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
                self.status = format!("读取配置失败: {e}");
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
            status: "读取配置成功；←/↑↓ 侧栏，Tab 表单，Ctrl+S 保存".into(),
            models_hint: "正在读取模型列表…".into(),
            search_hint: match self.conn.client.call("search.status", json!({})) {
                Ok(v) => {
                    let p = v["provider"].as_str().unwrap_or("?");
                    if p == "local" {
                        if v["configured"].as_bool().unwrap_or(false) {
                            "联网：local · 本地公开网页 · 免费无需 Key".to_string()
                        } else {
                            "联网：local · 插件未就绪".to_string()
                        }
                    } else if p == "none" {
                        "联网：已关闭".to_string()
                    } else {
                        format!(
                            "联网：{} · {} · {}",
                            p,
                            v["key_env"].as_str().unwrap_or("?"),
                            if v["configured"].as_bool().unwrap_or(false) {
                                "已配置"
                            } else {
                                "未配置"
                            }
                        )
                    }
                }
                Err(e) => format!("联网状态读取失败: {e}"),
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
                    s.status = format!("读取插件侧栏失败: {e}");
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
                        "{name} → {}（运行进程已同步）",
                        if enabled { "停用" } else { "启用" }
                    );
                }
            }
            Err(e) => {
                if let Some(s) = &mut self.settings {
                    s.status = format!("启停失败: {e}");
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
                    Some(e) => format!("模型列表不可用（可手动输入）: {e}"),
                    None => format!("{} 个模型；←/→ 选择，或直接输入", models.len()),
                };
                (models, hint)
            }
            Err(e) => (Vec::new(), format!("模型列表读取失败（可手动输入）: {e}")),
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
                s.status = "温度/上下文/回合数必须是数字".into();
            }
            return;
        };
        if base_url.is_empty() || model.is_empty() {
            if let Some(s) = &mut self.settings {
                s.status = "Base URL 和模型名不能为空".into();
            }
            return;
        }
        if !(0.0..=2.0).contains(&temperature) {
            if let Some(s) = &mut self.settings {
                s.status = "温度必须在 0.00–2.00 之间".into();
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
                        "已保存并应用：{} / {} / T={:.2}；工作区改动需重启 serve 生效",
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
                    s.status = format!("保存失败: {e}");
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
                    self.status = "已关闭设置页（未保存的修改已丢弃）".into();
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
                s.status = "API Key 已清空；Ctrl+S 保存后生效".into();
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
                        field: 0,
                        status: format!("已读取 {name} 的声明；←/→ 调整，Enter 保存"),
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
        let (n, save_idx, cancel_idx, field) = match &self.settings {
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
                s.status = "已返回侧栏（未保存的修改已丢弃）".into();
            }
            return;
        }
        if code == KeyCode::Enter {
            if field == save_idx {
                self.save_plugin_settings();
            } else if field == cancel_idx {
                if let Some(s) = &mut self.settings {
                    s.plugin = None;
                    s.detail = false;
                    s.nav_focus = true;
                }
            } else if field == 0 {
                if let Some(s) = &mut self.settings {
                    if let Some(p) = &mut s.plugin {
                        p.enabled = !p.enabled;
                        p.dirty = true;
                    }
                }
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
                                    "{} 必须是整数",
                                    f["label"].as_str().unwrap_or("该字段")
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
                                    "{} 必须是数字",
                                    f["label"].as_str().unwrap_or("该字段")
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
                            p.status = format!("保存失败: {e}");
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
                        p.status = format!("设置已保存，但启停失败: {e}");
                    }
                }
                return;
            }
        }
        if let Some(s) = &mut self.settings {
            if let Some(p) = &mut s.plugin {
                p.status = format!(
                    "已保存 {name} 的设置{}",
                    if enabled != enabled0 {
                        if enabled {
                            "，并已启用"
                        } else {
                            "，并已停用"
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
        if let Some((token, tool, risk)) = self.confirm.clone() {
            match code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    let _ = self
                        .conn
                        .client
                        .call("tool.confirm", json!({"token": token, "allow": true}));
                    self.status = format!("已允许 {tool}");
                    self.confirm = None;
                    return;
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    let _ = self.conn.client.call("tool.deny", json!({"token": token}));
                    self.status = format!("已拒绝 {tool}");
                    self.confirm = None;
                    return;
                }
                _ => {}
            }
            self.status = format!("待确认（{risk}）: {tool} — y 允许 / n 拒绝");
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
                    self.status = "会话列表：↑↓ 选择，Enter 进入，Esc 退出".into();
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
                    self.status = "先按 Enter 进入会话，再输入消息".into();
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
                    self.status = "用法: /use <session>".into();
                } else {
                    self.resume(arg);
                }
            }
            "workspace" | "project" => {
                if arg.is_empty() {
                    self.status = "用法: /workspace <项目名>；项目位于 workspace_root 下一层".into();
                } else {
                    match self
                        .conn
                        .client
                        .call("workspace.create", json!({"name": arg}))
                    {
                        Ok(v) => self.status = format!(
                            "已创建项目 {}（{}）；/new <会话> {} 可绑定",
                            v["name"].as_str().unwrap_or(arg),
                            v["path"].as_str().unwrap_or(""),
                            arg
                        ),
                        Err(e) => self.status = format!("创建项目失败: {e}"),
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
                        "系统".into(),
                        format!("项目容器 {}\n{text}", v["root"].as_str().unwrap_or("")),
                    ));
                }
                Err(e) => self.status = format!("读取项目失败: {e}"),
            },
            "fork" => {
                let Some(id) = self.session.clone() else {
                    self.status = "先 /new 或 /use".into();
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
                        self.status = format!("已 fork → {target}（复制 {} 条）", v["copied"]);
                        self.refresh_sessions();
                    }
                    Err(e) => self.status = format!("fork 失败: {e}"),
                }
            }
            "stop" | "cancel" => {
                if let Some(id) = &self.session {
                    match self.conn.client.call("chat.stop", json!({"session": id})) {
                        Ok(_) => self.status = "已紧急停止；已生成内容与上下文已保存在会话日志".into(),
                        Err(e) => self.status = format!("停止失败: {e}"),
                    }
                }
            }
            "steer" | "inject" => {
                let Some(id) = self.session.clone() else {
                    self.status = "先 /new 或 /use".into();
                    return;
                };
                let text = if rest.is_empty() { arg.to_string() } else { format!("{arg} {rest}") };
                if text.trim().is_empty() {
                    self.status = "用法: /steer <插入给模型的引导>".into();
                    return;
                }
                match self
                    .conn
                    .client
                    .call("chat.inject", json!({"session": id, "text": text}))
                {
                    Ok(_) => {
                        self.messages.push(("你（引导）".into(), text));
                        self.status = "引导已写入上下文；当前生成会在边界中断并重新开始".into();
                    }
                    Err(e) => self.status = format!("插入引导失败: {e}"),
                }
            }
            "compact" => {
                let Some(id) = self.session.clone() else {
                    self.status = "先 /new 或 /use".into();
                    return;
                };
                match self.conn.client.call("context.compact", json!({"session": id})) {
                    Ok(v) => {
                        self.messages.push((
                            "系统".into(),
                            format!(
                                "已压缩上下文：{} 事件 → 摘要 {} 字符（覆盖至 #{}）",
                                v["compacted_events"], v["summary_chars"], v["covers_until_seq"]
                            ),
                        ));
                        self.status = "上下文压缩完成；旧原文仍保留在会话日志".into();
                        self.refresh_ctx();
                    }
                    Err(e) => self.status = format!("压缩失败: {e}"),
                }
            }
            "settings" | "config" => self.open_settings(),
            "plugins" => {
                self.refresh_plugins();
                self.status = "插件管理：Space 启停，r 刷新，Esc 返回".into();
            }
            "clear" => self.messages.clear(),
            "help" => self.messages.push((
                "系统".into(),
                "/new [id] [项目] /workspace <项目> /workspaces /use <id> /fork [id]\n/stop /steer <引导> /compact /settings /plugins /clear /quit\n运行中直接输入文本=插入引导；/stop=紧急停止并保存上下文".into(),
            )),
            _ => self.status = format!("未知命令 /{name}"),
        }
    }

    fn send(&mut self, text: &str) {
        let Some(id) = self.session.clone() else {
            self.status = "先 /new 或 /use 选择会话".into();
            return;
        };
        self.focus = Focus::Chat;
        self.chat_scroll = 0;
        let running = matches!(
            self.state.as_str(),
            "running" | "assembling" | "generating" | "tool_executing"
        );
        if running {
            self.messages.push(("你（引导）".into(), text.to_string()));
            match self
                .conn
                .client
                .call("chat.inject", json!({"session": id, "text": text}))
            {
                Ok(_) => self.status = "引导已插入上下文；当前生成中断后继续".into(),
                Err(e) => {
                    self.messages
                        .push(("系统".into(), format!("插入引导失败: {e}")));
                    self.status = e;
                }
            }
            return;
        }
        self.messages.push(("你".into(), text.to_string()));
        match self
            .conn
            .client
            .call("chat.send", json!({"session": id, "text": text}))
        {
            Ok(_) => {
                self.state = "running".into();
                self.status = format!("发送中…（{id}）");
            }
            Err(e) => {
                self.messages
                    .push(("系统".into(), format!("发送失败: {e}")));
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
                    self.confirm = Some((token, tool, risk));
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
                        self.messages.push(("你（引导）".into(), t.to_string()));
                    }
                }
                "model.message" => {
                    let t = p.get("text").and_then(|x| x.as_str()).unwrap_or("");
                    self.messages.push(("助手".into(), t.to_string()));
                }
                "tool.call" => {
                    let t = p.get("tool").and_then(|x| x.as_str()).unwrap_or("?");
                    self.messages.push(("工具".into(), format!("请求 {t}")));
                }
                "tool.result" => {
                    let t = p.get("name").and_then(|x| x.as_str()).unwrap_or("?");
                    let r = p.get("result").cloned().unwrap_or(Value::Null);
                    let brief = r.to_string().chars().take(200).collect::<String>();
                    self.messages
                        .push(("工具".into(), format!("{t} → {brief}")));
                }
                "tool.error" => {
                    let t = p.get("name").and_then(|x| x.as_str()).unwrap_or("?");
                    self.messages.push((
                        "工具".into(),
                        format!(
                            "{t} 业务错误: {}",
                            p.get("error").and_then(|x| x.as_str()).unwrap_or("")
                        ),
                    ));
                }
                "tool.failed" => {
                    let t = p.get("name").and_then(|x| x.as_str()).unwrap_or("?");
                    self.messages.push((
                        "工具".into(),
                        format!(
                            "{t} 插件失败: {}",
                            p.get("error").and_then(|x| x.as_str()).unwrap_or("")
                        ),
                    ));
                }
                "tool.degraded" => {
                    let t = p.get("name").and_then(|x| x.as_str()).unwrap_or("?");
                    let n = p.get("failures").and_then(|x| x.as_u64()).unwrap_or(3);
                    self.messages.push((
                        "系统".into(),
                        format!("工具已熔断: {t}（连续 {n} 次技术失败）"),
                    ));
                }
                "config.changed" => {
                    self.messages.push(("系统".into(), "内核配置已更新".into()));
                }
                "context.compacted" => {
                    let n = p
                        .get("compacted_events")
                        .and_then(|x| x.as_u64())
                        .unwrap_or(0);
                    let chars = p.get("after_chars").and_then(|x| x.as_u64()).unwrap_or(0);
                    self.messages.push((
                        "系统".into(),
                        format!("上下文已压缩：{n} 个旧事件 → {chars} 字符摘要（原文仍在日志）"),
                    ));
                    self.refresh_ctx();
                }
                "permission.denied" => {
                    self.messages.push((
                        "系统".into(),
                        format!(
                            "拒绝: {}",
                            p.get("reason").and_then(|x| x.as_str()).unwrap_or("")
                        ),
                    ));
                }
                "plugin.degraded" => {
                    self.messages
                        .push(("系统".into(), "插件降级（连续失败 3 次）".into()));
                }
                "subscriber.lagged" => {
                    self.messages
                        .push(("系统".into(), "事件订阅落后，已丢弃旧事件".into()));
                }
                "chat.finished" => {
                    let s = p.get("status").and_then(|x| x.as_str()).unwrap_or("done");
                    self.messages
                        .push(("系统".into(), format!("— 本轮结束（{s}）")));
                    self.state = if s == "cancelled" || s == "stopped" {
                        "cancelled".into()
                    } else {
                        "idle".into()
                    };
                    self.status = format!("完成（{s}）");
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
                            .push(("系统".into(), format!("召回 {n} 条相关记忆")));
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
        "user.message" => ("你".to_string(), p.get("text")?.as_str()?.to_string()),
        "model.message" => {
            let t = p.get("text").and_then(|x| x.as_str()).unwrap_or("");
            if t.is_empty() {
                return None;
            }
            ("助手".to_string(), t.to_string())
        }
        "tool.requested" => (
            "工具".to_string(),
            format!(
                "请求 {}",
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
            ("工具".to_string(), format!("{name} → {brief}"))
        }
        "tool.error" => (
            "工具".to_string(),
            format!(
                "{} 业务错误: {}",
                p.get("name").and_then(|x| x.as_str()).unwrap_or("?"),
                p.get("error").and_then(|x| x.as_str()).unwrap_or("")
            ),
        ),
        "tool.failed" => (
            "工具".to_string(),
            format!(
                "{} 插件失败: {}",
                p.get("name").and_then(|x| x.as_str()).unwrap_or("?"),
                p.get("error").and_then(|x| x.as_str()).unwrap_or("")
            ),
        ),
        "permission.denied" => (
            "系统".to_string(),
            format!(
                "拒绝: {}",
                p.get("reason").and_then(|x| x.as_str()).unwrap_or("")
            ),
        ),
        "chat.finished" => (
            "系统".to_string(),
            format!(
                "— 本轮结束（{}）",
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

fn draw(f: &mut Frame, app: &App) {
    if app.plugins.is_some() {
        draw_plugins(f, app);
        return;
    }
    if app.settings.is_some() {
        draw_settings(f, app);
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
        Focus::Sessions => "会话列表",
        Focus::Chat => "对话",
    };
    let head = format!(
        " FeatherAgent v{}  {}  会话: {}  状态: {}  |  {}",
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
            let text = format!("{id}\n{ws} · {n} 条 {live}");
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
                "会话 ↑↓ / Enter"
            } else {
                "会话（Esc 返回）"
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
            "你" => Color::Green,
            "助手" => Color::Cyan,
            "工具" => Color::Yellow,
            "系统" => Color::DarkGray,
            _ => Color::White,
        };
        if role == "助手" {
            lines.push(Line::from(Span::styled(
                "助手:".to_string(),
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
    let scroll = max_scroll.saturating_sub(app.chat_scroll.min(max_scroll));
    let visible: Vec<Line> = lines.into_iter().skip(scroll).take(h).collect();
    f.render_widget(
        Paragraph::new(visible).block(Block::default().borders(Borders::ALL).title(
            if app.focus == Focus::Chat {
                "对话 ↑↓ 翻阅 · Esc 返回列表"
            } else {
                "对话（Enter 进入会话）"
            },
        )),
        cols[1],
    );

    // 右：上下文面板
    let mut ctx_lines = Vec::new();
    if app.ctx.is_null() {
        ctx_lines.push(Line::from("（尚无快照，发送一条消息后出现）"));
    } else {
        let st = &app.ctx["stats"];
        let window = st["window_tokens"].as_u64().unwrap_or(8192) as usize;
        let total_tokens = st["total_tokens"].as_u64().unwrap_or(0) as usize;
        let pct = total_tokens
            .saturating_mul(100)
            .checked_div(window)
            .unwrap_or(0);
        ctx_lines.push(Line::from(format!("预算 {total_tokens}/{window} ({pct}%)")));
        ctx_lines.push(Line::from(format!(
            "分层 L1 {} 条 / L2 {} 卡 / L3 {} tok / L4 {} 项",
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
            "模型 {} · 事件 {}",
            app.ctx["model"].as_str().unwrap_or("-"),
            app.ctx["turns"].as_u64().unwrap_or(0),
        )));
        let goal = app.ctx["task_state"]["goal"].as_str().unwrap_or("");
        if !goal.is_empty() {
            ctx_lines.push(Line::from(format!("目标: {goal}")));
        }
        let cards = app.ctx["cards"].as_u64().unwrap_or(0);
        ctx_lines.push(Line::from(format!("摘要卡 {cards} 张")));
    }
    if let Some((_, tool, risk)) = &app.confirm {
        ctx_lines.push(Line::from(""));
        ctx_lines.push(Line::from(Span::styled(
            format!("⚠ 待确认 [{risk}] {tool}"),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
        ctx_lines.push(Line::from("y 允许 / n 拒绝"));
    }
    f.render_widget(
        Paragraph::new(ctx_lines)
            .block(Block::default().borders(Borders::ALL).title("上下文"))
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
        "输入（Enter 发送，←/→ 移动光标，/help 命令）"
    } else {
        "输入（会话列表：Enter 进入，↑↓ 选择，Esc 退出）"
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
            format!(" FeatherAgent {} 设置 ", featheragent::VERSION),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(if s.nav_focus {
            "  ↑↓ 侧栏 · Enter 打开 · Tab 到表单 · Esc 关闭"
        } else if s.detail {
            "  Space/Enter 启停 · r 刷新 · Esc 返回侧栏"
        } else {
            "  Tab/↑↓ 切字段 · ←/→ 选 provider/model/调数值 · Esc 回侧栏"
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
        "{} ⚙ 常规设置",
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
        "插件页面",
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
        "插件由内核登记表生成",
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(
        Paragraph::new(nav_lines)
            .block(Block::default().borders(Borders::ALL).title("侧栏"))
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
                    "状态      {}",
                    if enabled { "已启用" } else { "已停用" }
                )),
                Line::from(format!("live      {live}")),
                Line::from(format!("版本      {version}")),
                Line::from(format!("capabilities  {}", caps)),
                Line::from(format!("permissions   {}", perms)),
                Line::from(format!("program   {program}")),
                Line::from(format!("blake3    {hash}")),
                Line::from(""),
                Line::from(if s.detail {
                    "Space/Enter 启停 · r 刷新 · Esc 返回侧栏"
                } else {
                    "Enter 打开详情"
                }),
            ];
            f.render_widget(
                Paragraph::new(lines)
                    .block(Block::default().borders(Borders::ALL).title("插件详情"))
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
                    "(未设置；本地服务通常可留空)".into()
                } else if s.field == 3 && !s.nav_focus {
                    format!("已填写 {} 字符（Ctrl+U 清空）", s.api_key.chars().count())
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
                    "(安装根/workspace：项目容器)".into()
                } else {
                    s.workspace_root.clone()
                },
            ),
            (
                "Auto allow safe",
                if s.auto_allow_safe {
                    "[x] 开".into()
                } else {
                    "[ ] 关".into()
                },
            ),
            ("保存并应用", "按 Enter / Ctrl+S".into()),
            ("取消/返回", "Esc 回侧栏".into()),
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
            "插件自己的设置在左侧对应插件页里（由插件声明，内核转述）",
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
                        .title("常规（保存后提交 config.set）"),
                )
                .wrap(Wrap { trim: false })
                .scroll((offset, 0)),
            body[1],
        );
    }

    let plugin_status = s.plugin.as_ref().map(|p| p.status.clone());
    let status = match (&plugin_status, s.status.is_empty()) {
        (Some(p), _) => p.as_str(),
        (None, true) => "设置页已打开",
        (None, false) => s.status.as_str(),
    };
    f.render_widget(
        Paragraph::new(status)
            .style(Style::default().fg(Color::Yellow))
            .block(Block::default().borders(Borders::ALL).title("状态")),
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
            if enabled { "已启用" } else { "已停用" }
        )),
        if p.dirty {
            Span::styled("  未保存", Style::default().fg(Color::Yellow))
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
                "启用插件",
                if p.enabled { "[x] 开" } else { "[ ] 关" }
            )
        } else if i == p.save_idx() {
            format!("{marker}{:<16} Enter / Ctrl+S", "保存并应用")
        } else if i == p.cancel_idx() {
            format!("{marker}{:<16} Esc", "取消/返回")
        } else {
            let f = &p.fields[i - 1];
            let label = f.get("label").and_then(|x| x.as_str()).unwrap_or("?");
            let kind = f.get("type").and_then(|x| x.as_str()).unwrap_or("string");
            let shown = match kind {
                "bool" => {
                    if p.values[i - 1].as_bool().unwrap_or(false) {
                        "[x] 开".to_string()
                    } else {
                        "[ ] 关".to_string()
                    }
                }
                "enum" => p.values[i - 1].as_str().unwrap_or("").to_string(),
                _ => {
                    let t = &p.text[i - 1];
                    if t.trim().is_empty() {
                        f.get("placeholder")
                            .and_then(|x| x.as_str())
                            .unwrap_or("(空)")
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
                    .title(format!("{}（由插件声明）", p.title)),
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
            format!(" FeatherAgent v{} 插件管理 ", env!("CARGO_PKG_VERSION")),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  ↑↓ 选择 · Space/Enter 启停 · r 刷新 · Esc 返回"),
    ]));
    f.render_widget(head, chunks[0]);

    let mut lines = Vec::new();
    if p.items.is_empty() {
        lines.push(Line::from(
            "（没有已登记插件；可通过 WebUI 插件页按路径安装）",
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
                    .title("插件（启停会同步到运行进程）"),
            )
            .wrap(Wrap { trim: false }),
        chunks[1],
    );
    f.render_widget(
        Paragraph::new(p.status.as_str())
            .style(Style::default().fg(Color::Yellow))
            .block(Block::default().borders(Borders::ALL).title("状态")),
        chunks[2],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_display_width_is_two_columns() {
        assert_eq!(display_width("你好"), 4);
        assert_eq!(display_width("a你b"), 4);
        assert_eq!(display_width("abc"), 3);
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn markdown_headings_lists_code_are_rendered_without_markers() {
        let text = "# 标题\n\n- **粗体** 与 `code`\n1. 第一项\n\n```rust\nfn main() {}\n```\n\n[链接](https://example.com)";
        let lines = markdown_lines(text, 30, Color::Cyan);
        let joined: String = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
        assert!(joined.contains("标题"));
        assert!(!joined.contains("**"));
        assert!(!joined.contains("`code`"));
        assert!(joined.contains("code"));
        assert!(joined.contains("fn main()"));
        assert!(joined.contains("https://example.com"));
        assert!(!joined.contains("](https://"));
        assert!(joined.contains("1. 第一项"));
        assert!(joined.contains("╰─"));
    }

    #[test]
    fn wrap_display_respects_width_and_prefix() {
        let lines = wrap_display("你: ", "abcdef", 6);
        assert_eq!(lines[0], "你: ab");
        assert_eq!(lines[1], "cdef");
        assert!(lines.iter().all(|l| display_width(l) <= 6));
    }
}
