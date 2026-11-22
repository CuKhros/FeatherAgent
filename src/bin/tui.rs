//! `featheragent-tui`：三栏终端前端（§7.3）。
//!
//! 左：会话列表；中：对话流；右：上下文面板（预算分层 / 任务态 / 模型）；
//! 底：输入行。危险工具确认用 y/n 应答。所有数据走 UDS 契约，不读内核内存。

use std::io::{stdout, Stdout};
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Terminal;
use serde_json::{json, Value};

use featheragent::governance::paths::{Overrides, Paths};
use featheragent::server::client::{Client, Connect};
use featheragent::util::Args;

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
        match self.conn.client.call("session.resume", json!({"session": id})) {
            Ok(v) => {
                self.session = Some(id.to_string());
                self.state = v["state"].as_str().unwrap_or("idle").to_string();
                self.messages.clear();
                self.messages.push(("系统".into(), format!("已恢复会话 {id}（{} 条事件）", v["total"])));
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

    fn on_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        // 危险工具确认优先
        if let Some((token, tool, risk)) = self.confirm.clone() {
            match code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    let _ = self.conn.client.call("tool.confirm", json!({"token": token, "allow": true}));
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
                let target = if arg.is_empty() { format!("{id}-fork") } else { arg.to_string() };
                match self.conn.client.call("session.fork", json!({"session": id, "as": target})) {
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
            "clear" => self.messages.clear(),
            "help" => self.messages.push((
                "系统".into(),
                "/new [id] /use <id> /fork [id] /cancel /clear /quit".into(),
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
        match self.conn.client.call("chat.send", json!({"session": id, "text": text})) {
            Ok(_) => {
                self.state = "running".into();
                self.status = format!("发送中…（{id}）");
            }
            Err(e) => {
                self.messages.push(("系统".into(), format!("发送失败: {e}")));
                self.status = e;
            }
        }
    }

    fn drain_events(&mut self) {
        while let Ok(v) = self.conn.events.try_recv() {
            if v.get("method").and_then(|m| m.as_str()) != Some("event") {
                if v.get("method").and_then(|m| m.as_str()) == Some("tool.confirm_request") {
                    let token = v.pointer("/params/token").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let tool = v.pointer("/params/tool").and_then(|x| x.as_str()).unwrap_or("?").to_string();
                    let risk = v.pointer("/params/risk").and_then(|x| x.as_str()).unwrap_or("dangerous").to_string();
                    self.confirm = Some((token, tool, risk));
                }
                continue;
            }
            let kind = v.pointer("/params/kind").and_then(|x| x.as_str()).unwrap_or("");
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
                    self.messages.push(("工具".into(), format!("{t} → {brief}")));
                }
                "tool.failed" => {
                    let t = p.get("name").and_then(|x| x.as_str()).unwrap_or("?");
                    self.messages.push(("工具".into(), format!("{t} 失败: {}", p.get("error").and_then(|x| x.as_str()).unwrap_or(""))));
                }
                "permission.denied" => {
                    self.messages.push(("系统".into(), format!("拒绝: {}", p.get("reason").and_then(|x| x.as_str()).unwrap_or(""))));
                }
                "plugin.degraded" => {
                    self.messages.push(("系统".into(), "插件降级（连续失败 3 次）".into()));
                }
                "subscriber.lagged" => {
                    self.messages.push(("系统".into(), "事件订阅落后，已丢弃旧事件".into()));
                }
                "chat.finished" => {
                    let s = p.get("status").and_then(|x| x.as_str()).unwrap_or("done");
                    self.messages.push(("系统".into(), format!("— 本轮结束（{s}）")));
                    self.state = if s == "cancelled" { "cancelled".into() } else { "idle".into() };
                    self.status = format!("完成（{s}）");
                    self.refresh_ctx();
                    self.refresh_sessions();
                }
                "memory.recalled" => {
                    let n = p.get("items").and_then(|x| x.as_array()).map(|a| a.len()).unwrap_or(0);
                    if n > 0 {
                        self.messages.push(("系统".into(), format!("召回 {n} 条相关记忆")));
                    }
                }
                _ => {}
            }
        }
    }
}

fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(6), Constraint::Length(3)])
        .split(f.size());

    let head = format!(
        " FeatherAgent v1.1  会话: {}  状态: {}  |  {}",
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
        .constraints([Constraint::Percentage(18), Constraint::Percentage(52), Constraint::Percentage(30)])
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
            let style = if i == app.selected { Style::default().bg(Color::DarkGray) } else { Style::default() };
            ListItem::new(text).style(style)
        })
        .collect();
    let mut list_state = ListState::default().with_selected(Some(app.selected));
    f.render_stateful_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title("会话 ↑↓ / Enter")),
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
            let prefix = if i == 0 { format!("{role}: ") } else { "   ".to_string() };
            lines.push(Line::from(vec![
                Span::styled(prefix, Style::default().fg(color).add_modifier(Modifier::BOLD)),
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
        let pct = if window == 0 { 0 } else { total_tokens * 100 / window };
        ctx_lines.push(Line::from(format!("预算 {total_tokens}/{window} ({pct}%)")));
        ctx_lines.push(Line::from(format!("分层 L1 {} 条 / L2 {} 卡 / L3 {} tok / L4 {} 项",
            st["layers"]["l1_window_messages"].as_u64().unwrap_or(0),
            st["layers"]["l2_summary_cards"].as_u64().unwrap_or(0),
            st["layers"]["l3_task_state_tokens"].as_u64().unwrap_or(0),
            st["layers"]["l4_recalled_items"].as_u64().unwrap_or(0),
        )));
        ctx_lines.push(Line::from(format!("system {} · tools {} · skills {} · history {} · recall {}",
            st["system_tokens"].as_u64().unwrap_or(0),
            st["tools_tokens"].as_u64().unwrap_or(0),
            st["skills_tokens"].as_u64().unwrap_or(0),
            st["history_tokens"].as_u64().unwrap_or(0),
            st["recall_tokens"].as_u64().unwrap_or(0),
        )));
        ctx_lines.push(Line::from(format!("模型 {} · 事件 {}",
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
        Paragraph::new(input)
            .block(Block::default().borders(Borders::ALL).title("输入（Enter 发送，/help 命令）")),
        chunks[2],
    );
    let x = chunks[2].x + 2 + app.input.chars().count() as u16;
    let y = chunks[2].y + 1;
    f.set_cursor(x.min(chunks[2].right().saturating_sub(1)), y);
}
