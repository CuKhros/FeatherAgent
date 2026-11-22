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
    temperature: String,
    context_window: String,
    max_turns: String,
    auto_allow_safe: bool,
    field: usize,
    status: String,
    models_hint: String,
}

const SETTINGS_FIELDS: usize = 9;

struct App {
    conn: Connect,
    sessions: Vec<Value>,
    selected: usize,
    messages: Vec<(String, String)>,
    session: Option<String>,
    state: String,
    ctx: Value,
    input: String,
    status: String,
    confirm: Option<(String, String, String)>, // token, tool, risk
    settings: Option<SettingsState>,
    quit: bool,
}

fn main() {
    let args = Args::parse(std::env::args().skip(1));
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
        status: format!("已连接 {}", socket.display()),
        confirm: None,
        settings: None,
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
                self.messages.push((
                    "系统".into(),
                    format!("已恢复会话 {id}（{} 条事件）", v["total"]),
                ));
                self.status = format!("会话 {id}");
                self.refresh_ctx();
            }
            Err(e) => self.status = format!("恢复失败: {e}"),
        }
    }

    fn create(&mut self, id: &str) {
        match self.conn.client.call("session.create", json!({"id": id})) {
            Ok(_) => {
                self.session = Some(id.to_string());
                self.messages.clear();
                self.messages.push(("系统".into(), format!("新会话 {id}")));
                self.refresh_sessions();
                self.refresh_ctx();
            }
            Err(e) => self.status = format!("创建失败: {e}"),
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
            auto_allow_safe: cfg
                .get("auto_allow_safe")
                .and_then(|x| x.as_bool())
                .unwrap_or(true),
            field: 2,
            status: "读取配置成功；Tab 切换字段，Ctrl+S 保存并应用".into(),
            models_hint: "正在读取模型列表…".into(),
        };
        self.settings = Some(state);
        self.refresh_settings_models(false);
    }

    fn refresh_settings_models(&mut self, prefer_first: bool) {
        let (provider, base_url, model) = match &self.settings {
            Some(s) => (s.provider.clone(), s.base_url.clone(), s.model.clone()),
            None => return,
        };
        let (models, hint) = match self.conn.client.call(
            "provider.models",
            json!({"provider": provider, "base_url": base_url, "model": model}),
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
        let (provider, base_url, model, temperature, context_window, max_turns, auto_allow_safe) =
            match &self.settings {
                Some(s) => (
                    s.provider.clone(),
                    s.base_url.trim().to_string(),
                    s.model.trim().to_string(),
                    s.temperature.trim().parse::<f64>(),
                    s.context_window.trim().parse::<u64>(),
                    s.max_turns.trim().parse::<u64>(),
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
            "temperature": temperature,
            "context_window": context_window,
            "max_turns": max_turns,
            "auto_allow_safe": auto_allow_safe,
        });
        match self.conn.client.call("config.set", json!({"patch": patch})) {
            Ok(v) => {
                let cfg = v.get("config").cloned().unwrap_or(Value::Null);
                if let Some(s) = &mut self.settings {
                    s.status = format!(
                        "已保存并应用：{} / {} / T={:.2}",
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
            self.save_settings();
            return;
        }
        let field = match &self.settings {
            Some(s) => s.field,
            None => return,
        };
        match code {
            KeyCode::Esc => {
                self.settings = None;
                self.status = "已关闭设置页（未保存的修改已丢弃）".into();
            }
            KeyCode::Enter => {
                if field == 8 {
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
                            refresh = true;
                        }
                        2 if !s.models.is_empty() => {
                            let n = s.models.len() as i32;
                            s.model_idx = ((s.model_idx as i32 + dir + n) % n) as usize;
                            s.model = s.models[s.model_idx].clone();
                        }
                        3 => {
                            let v = s.temperature.parse::<f64>().unwrap_or(0.7);
                            s.temperature =
                                format!("{:.2}", (v + dir as f64 * 0.05).clamp(0.0, 2.0));
                        }
                        4 => {
                            let v = s.context_window.parse::<u64>().unwrap_or(8192) as i64;
                            s.context_window =
                                (v + dir as i64 * 256).clamp(512, 131072).to_string();
                        }
                        5 => {
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
                        3 => Some(&mut s.temperature),
                        4 => Some(&mut s.context_window),
                        5 => Some(&mut s.max_turns),
                        _ => None,
                    };
                    if let Some(t) = target {
                        t.pop();
                    }
                }
            }
            KeyCode::Char(' ') if field == 6 => {
                if let Some(s) = &mut self.settings {
                    s.auto_allow_safe = !s.auto_allow_safe;
                }
            }
            KeyCode::Char(c) => {
                if let Some(s) = &mut self.settings {
                    let target = match field {
                        1 => Some(&mut s.base_url),
                        2 => Some(&mut s.model),
                        3 => Some(&mut s.temperature),
                        4 => Some(&mut s.context_window),
                        5 => Some(&mut s.max_turns),
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
        match code {
            KeyCode::Char('c') if mods.contains(KeyModifiers::CONTROL) => self.quit = true,
            KeyCode::Esc => self.quit = true,
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Down => {
                if self.selected + 1 < self.sessions.len() {
                    self.selected += 1;
                }
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Enter => {
                let text = self.input.trim().to_string();
                self.input.clear();
                if text.is_empty() {
                    if let Some(s) = self
                        .sessions
                        .get(self.selected)
                        .and_then(|x| x["id"].as_str())
                        .map(|x| x.to_string())
                    {
                        self.resume(&s);
                    }
                    return;
                }
                if let Some(cmd) = text.strip_prefix('/') {
                    self.command(cmd);
                } else {
                    self.send(&text);
                }
            }
            KeyCode::Char(c) => self.input.push(c),
            _ => {}
        }
    }

    fn command(&mut self, cmd: &str) {
        let mut it = cmd.split_whitespace();
        let name = it.next().unwrap_or("");
        let arg = it.next().unwrap_or("");
        match name {
            "quit" | "q" => self.quit = true,
            "new" => self.create(if arg.is_empty() { "s-tui" } else { arg }),
            "use" | "resume" => {
                if arg.is_empty() {
                    self.status = "用法: /use <session>".into();
                } else {
                    self.resume(arg);
                }
            }
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
            "cancel" => {
                if let Some(id) = &self.session {
                    match self.conn.client.call("chat.cancel", json!({"session": id})) {
                        Ok(_) => self.status = "已请求取消".into(),
                        Err(e) => self.status = format!("取消失败: {e}"),
                    }
                }
            }
            "settings" | "config" => self.open_settings(),
            "clear" => self.messages.clear(),
            "help" => self.messages.push((
                "系统".into(),
                "/new [id] /use <id> /fork [id] /cancel /settings /clear /quit".into(),
            )),
            _ => self.status = format!("未知命令 /{name}"),
        }
    }

    fn send(&mut self, text: &str) {
        let Some(id) = self.session.clone() else {
            self.status = "先 /new 或 /use 选择会话".into();
            return;
        };
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
                // user.message 自己发的已在本地回显；其他客户端的输入不重复追加
                "user.message" => {}
                "model.message" => {
                    let t = p.get("text").and_then(|x| x.as_str()).unwrap_or("");
                    self.messages.push(("助手".into(), t.to_string()));
                }
                "tool.requested" => {
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
                    self.state = if s == "cancelled" {
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

fn draw(f: &mut Frame, app: &App) {
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

    let head = format!(
        " FeatherAgent v1.1.6  会话: {}  状态: {}  |  {}",
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
            let text = format!("{id}\n{n} 条 {live}");
            let style = if i == app.selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };
            ListItem::new(text).style(style)
        })
        .collect();
    let mut list_state = ListState::default().with_selected(Some(app.selected));
    f.render_stateful_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title("会话 ↑↓ / Enter"),
        ),
        cols[0],
        &mut list_state,
    );

    // 中：对话流
    let width = cols[1].width.saturating_sub(2) as usize;
    let mut lines: Vec<Line> = Vec::new();
    let mut total = 0usize;
    for (role, text) in &app.messages {
        let color = match role.as_str() {
            "你" => Color::Green,
            "助手" => Color::Cyan,
            "工具" => Color::Yellow,
            "系统" => Color::DarkGray,
            _ => Color::White,
        };
        for (i, para) in text.split('\n').enumerate() {
            let prefix = if i == 0 {
                format!("{role}: ")
            } else {
                "   ".to_string()
            };
            lines.push(Line::from(vec![
                Span::styled(
                    prefix,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(para.to_string()),
            ]));
            total += 1 + para.chars().count() / width.max(1);
        }
        total += 1;
    }
    let h = cols[1].height.saturating_sub(2) as usize;
    let scroll = total.saturating_sub(h) as u16;
    f.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title("对话"))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        cols[1],
    );

    // 右：上下文面板
    let mut ctx_lines = Vec::new();
    if app.ctx.is_null() {
        ctx_lines.push(Line::from("（尚无快照，发送一条消息后出现）"));
    } else {
        let st = &app.ctx["stats"];
        let window = 8192usize;
        let total_tokens = st["total_tokens"].as_u64().unwrap_or(0) as usize;
        let pct = if window == 0 {
            0
        } else {
            total_tokens * 100 / window
        };
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

    // 底：输入
    let input = format!("› {}", app.input);
    f.render_widget(
        Paragraph::new(input).block(
            Block::default()
                .borders(Borders::ALL)
                .title("输入（Enter 发送，/help 命令）"),
        ),
        chunks[2],
    );
    let x = chunks[2].x + 2 + app.input.chars().count() as u16;
    let y = chunks[2].y + 1;
    f.set_cursor(x.min(chunks[2].right().saturating_sub(1)), y);
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
            " FeatherAgent v1.1.6 设置 ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(
            "  Tab/↑↓ 切字段 · ←/→ 选 provider/model/调数值 · Ctrl+S 或 Enter 保存 · Esc 返回",
        ),
    ]));
    f.render_widget(head, chunks[0]);

    let values = [
        ("Provider", s.provider.clone()),
        ("Base URL", s.base_url.clone()),
        ("Model", s.model.clone()),
        ("Temperature", format!("{}（0.00–2.00）", s.temperature)),
        ("Context window", format!("{} tokens", s.context_window)),
        ("Max turns", s.max_turns.clone()),
        (
            "Auto allow safe",
            if s.auto_allow_safe {
                "[x] 开".into()
            } else {
                "[ ] 关".into()
            },
        ),
        ("保存并应用", "按 Enter / Ctrl+S".into()),
        ("取消", "按 Enter 关闭".into()),
    ];
    let mut lines = Vec::new();
    for (i, (label, val)) in values.iter().enumerate() {
        let marker = if i == s.field { "▶ " } else { "  " };
        let text = format!("{marker}{label:<16} {val}");
        let mut line = Line::from(text);
        if i == s.field {
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
        &s.models_hint,
        Style::default().fg(Color::DarkGray),
    )));
    for m in s.models.iter().take(12) {
        lines.push(Line::from(format!("  · {m}")));
    }
    f.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("配置（保存后提交 config.set）"),
            )
            .wrap(Wrap { trim: false }),
        chunks[1],
    );

    let status = if s.status.is_empty() {
        "设置页已打开"
    } else {
        s.status.as_str()
    };
    f.render_widget(
        Paragraph::new(status)
            .style(Style::default().fg(Color::Yellow))
            .block(Block::default().borders(Borders::ALL).title("状态")),
        chunks[2],
    );
}
