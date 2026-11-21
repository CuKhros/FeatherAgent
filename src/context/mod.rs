//! 上下文引擎（设计文档 §5）：预算器、四层压缩、容错解析、技能规范、多模态预算。
//!
//! 四层压缩：
//! - **L1 滑窗**：从最新往回装，装不下即停；
//! - **L2 摘要卡**：被裁掉的旧轮次压成卡片，附原文引用（可 pin）；
//! - **L3 task_state**：当前目标、未完成工具调用、最近失败置头；
//! - **L4 向量记忆召回**：由 storage 插件提供，按预算追加。
//!
//! KV Cache 友好：稳定前缀（system + task_state + 工具定义）永远置头，
//! 技能集与工具集在会话开始即锁定（§5.4）。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub mod parse;

use crate::proto::types::{Event, EventKind};

/// 单轮图像 token 上限（§5.5）。
pub const IMAGE_TOKENS_PER_ROUND: usize = 1500;
/// 单技能注入预算与总量预算（§5.6）。
pub const SKILL_TOKENS_EACH: usize = 400;
pub const SKILL_TOKENS_TOTAL: usize = 1200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Chat,
    Stream,
    Fim,
    Embed,
    Rerank,
    Tools,
    Json,
    Vision,
}

impl Capability {
    pub fn parse(s: &str) -> Option<Capability> {
        Some(match s {
            "chat" => Capability::Chat,
            "stream" => Capability::Stream,
            "fim" => Capability::Fim,
            "embed" => Capability::Embed,
            "rerank" => Capability::Rerank,
            "tools" => Capability::Tools,
            "json" => Capability::Json,
            "vision" => Capability::Vision,
            _ => return None,
        })
    }
}

/// 模型画像：上下文窗口、输出预留、能力位。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProfile {
    pub name: String,
    pub context_window: usize,
    pub max_output: usize,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl Default for ModelProfile {
    fn default() -> Self {
        Self {
            name: "qwen2.5:7b".into(),
            context_window: 8192,
            max_output: 1024,
            capabilities: vec!["chat".into(), "stream".into()],
        }
    }
}

impl ModelProfile {
    pub fn supports(&self, c: Capability) -> bool {
        self.capabilities
            .iter()
            .any(|x| Capability::parse(x) == Some(c))
    }
}

#[derive(Debug, Clone)]
pub struct CtxConfig {
    pub reserve_output: usize,
    pub max_tools: usize,
    pub summary_card_chars: usize,
    pub recall_items: usize,
}

impl Default for CtxConfig {
    fn default() -> Self {
        Self {
            reserve_output: 1024,
            max_tools: 12,
            summary_card_chars: 240,
            recall_items: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub refs: Vec<u64>,
}

impl Message {
    pub fn new(role: &str, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            pinned: false,
            refs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Layers {
    pub l1_window_messages: usize,
    pub l2_summary_cards: usize,
    pub l3_task_state_tokens: usize,
    pub l4_recalled_items: usize,
    pub media_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CtxStats {
    pub total_tokens: usize,
    pub window_tokens: usize,
    pub system_tokens: usize,
    pub task_state_tokens: usize,
    pub tools_tokens: usize,
    pub skills_tokens: usize,
    pub history_tokens: usize,
    pub summary_tokens: usize,
    pub recall_tokens: usize,
    pub layers: Layers,
    pub dropped_events: usize,
}

#[derive(Debug, Clone)]
pub struct Assembled {
    pub messages: Vec<Message>,
    pub stats: CtxStats,
    pub summary_cards: Vec<Value>,
    pub task_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallItem {
    pub id: String,
    pub text: String,
    pub score: f32,
    #[serde(default)]
    pub source: String,
}

#[derive(Debug)]
pub enum CtxError {
    TooManyTools(usize),
    OverBudget { need: usize, limit: usize },
}

impl std::fmt::Display for CtxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CtxError::TooManyTools(n) => write!(f, "工具集超过 12 个（{n}）"),
            CtxError::OverBudget { need, limit } => {
                write!(f, "上下文超预算：需要 {need} > {limit}")
            }
        }
    }
}

/// 粗略 token 估算：CJK 字符按 1，ASCII 按 1/4。
pub fn estimate_tokens(s: &str) -> usize {
    let mut cjk = 0usize;
    let mut ascii = 0usize;
    for c in s.chars() {
        if c.is_ascii() {
            ascii += 1;
        } else {
            cjk += 1;
        }
    }
    cjk + ascii / 4 + 1
}

pub struct ContextEngine {
    pub cfg: CtxConfig,
}

impl Default for ContextEngine {
    fn default() -> Self {
        Self::new(CtxConfig::default())
    }
}

impl ContextEngine {
    pub fn new(cfg: CtxConfig) -> Self {
        Self { cfg }
    }

    /// 组装一次请求的上下文。
    pub fn assemble(
        &self,
        events: &[Event],
        profile: &ModelProfile,
        tools: &[Value],
        skills: &[Skill],
        recalls: &[RecallItem],
    ) -> Result<Assembled, CtxError> {
        if tools.len() > self.cfg.max_tools {
            return Err(CtxError::TooManyTools(tools.len()));
        }
        let limit = profile.context_window.saturating_sub(
            profile
                .max_output
                .max(self.cfg.reserve_output)
                .min(profile.context_window / 2),
        );

        // ---- 稳定前缀 ----
        let system = format!(
            "你是 FeatherAgent 的助手，服务于本地 {} 模型。回答简洁、工具优先、不要臆造。",
            profile.name
        );
        let system_tokens = estimate_tokens(&system);

        let task_state = build_task_state(events);
        let task_state_tokens = estimate_tokens(&task_state);

        let tools_json = if tools.is_empty() {
            String::new()
        } else {
            let arr: Vec<&Value> = tools.iter().take(self.cfg.max_tools).collect();
            serde_json::to_string(&arr).unwrap_or_default()
        };
        let tools_tokens = estimate_tokens(&tools_json);

        let (skill_text, skill_tokens, skill_refs) = inject_skills(skills, &latest_user_text(events));
        let _ = skill_refs;

        // ---- 历史（L1） ----
        let mut history = events_to_messages(events);
        let mut kept: Vec<Message> = Vec::new();
        let mut history_tokens = 0usize;
        let mut dropped = 0usize;
        let base = system_tokens + task_state_tokens + tools_tokens + skill_tokens;
        while let Some(m) = history.pop() {
            let t = estimate_tokens(&m.content);
            if base + history_tokens + t > limit {
                dropped += 1;
                break;
            }
            history_tokens += t;
            kept.push(m);
        }
        dropped += history.len();
        kept.reverse();

        // ---- L2 摘要卡：被裁掉的前缀 ----
        let mut cards: Vec<Value> = Vec::new();
        let mut summary_tokens = 0usize;
        if dropped > 0 {
            let cut = events.len().min(dropped);
            let (card_text, refs) = summarize_prefix(&events[..cut], self.cfg.summary_card_chars);
            if !card_text.is_empty() {
                summary_tokens = estimate_tokens(&card_text);
                cards.push(json!({
                    "covers_until_seq": events.get(cut.saturating_sub(1)).map(|e| e.seq).unwrap_or(0),
                    "text": card_text,
                    "refs": refs,
                    "pinned": false,
                }));
            }
        }

        // ---- L4 召回 ----
        let mut recall_text = String::new();
        let mut recall_tokens = 0usize;
        let mut used_recall = 0usize;
        for r in recalls.iter().take(self.cfg.recall_items) {
            let line = format!("- [{}] {}", r.id, r.text);
            let t = estimate_tokens(&line);
            if recall_tokens + t > IMAGE_TOKENS_PER_ROUND || base + history_tokens + summary_tokens + recall_tokens + t > limit {
                break;
            }
            recall_tokens += t;
            recall_text.push_str(&line);
            recall_text.push('\n');
            used_recall += 1;
        }

        // ---- 多模态预算 ----
        let media_count = events
            .iter()
            .filter(|e| {
                e.kind == EventKind::UserMessage
                    && e.payload.get("media").is_some()
            })
            .count();
        let media_tokens = media_count.min(4) * (IMAGE_TOKENS_PER_ROUND / 4);

        let mut messages = Vec::new();
        messages.push(Message::new("system", &system));
        if !task_state.is_empty() {
            messages.push(Message::new("system", format!("[task_state]\n{task_state}")));
        }
        if !tools_json.is_empty() {
            messages.push(Message::new("system", format!("[tools]\n{tools_json}")));
        }
        if !skill_text.is_empty() {
            let mut m = Message::new("system", format!("[skills]\n{skill_text}"));
            m.pinned = true;
            messages.push(m);
        }
        if !recall_text.is_empty() {
            messages.push(Message::new("system", format!("[memory]\n{recall_text}")));
        }
        for c in &cards {
            let mut m = Message::new("system", format!("[summary] {}", c["text"].as_str().unwrap_or("")));
            m.refs = c["refs"].as_array().map(|a| a.iter().filter_map(|x| x.as_u64()).collect()).unwrap_or_default();
            messages.push(m);
        }
        let kept_count = kept.len();
        messages.extend(kept);

        let total = system_tokens
            + task_state_tokens
            + tools_tokens
            + skill_tokens
            + history_tokens
            + summary_tokens
            + recall_tokens
            + media_tokens;
        if total > profile.context_window {
            return Err(CtxError::OverBudget {
                need: total,
                limit: profile.context_window,
            });
        }

        Ok(Assembled {
            messages,
            stats: CtxStats {
                total_tokens: total,
                window_tokens: profile.context_window,
                system_tokens,
                task_state_tokens,
                tools_tokens,
                skills_tokens: skill_tokens,
                history_tokens,
                summary_tokens,
                recall_tokens,
                layers: Layers {
                    l1_window_messages: kept_count,
                    l2_summary_cards: cards.len(),
                    l3_task_state_tokens: task_state_tokens,
                    l4_recalled_items: used_recall,
                    media_tokens,
                },
                dropped_events: dropped,
            },
            summary_cards: cards,
            task_state,
        })
    }
}

fn latest_user_text(events: &[Event]) -> String {
    events
        .iter()
        .rev()
        .find(|e| e.kind == EventKind::UserMessage)
        .and_then(|e| e.payload.get("text").and_then(|t| t.as_str()).map(|s| s.to_string()))
        .unwrap_or_default()
}

fn events_to_messages(events: &[Event]) -> Vec<Message> {
    let mut out = Vec::new();
    for e in events {
        match e.kind {
            EventKind::UserMessage => {
                if let Some(t) = e.payload.get("text").and_then(|x| x.as_str()) {
                    out.push(Message::new("user", t));
                }
            }
            EventKind::ModelMessage => {
                if let Some(t) = e.payload.get("text").and_then(|x| x.as_str()) {
                    out.push(Message::new("assistant", t));
                }
            }
            EventKind::ToolCall => {
                let name = e.payload.get("name").and_then(|x| x.as_str()).unwrap_or("tool");
                let args = e.payload.get("arguments").cloned().unwrap_or(Value::Null);
                out.push(Message::new(
                    "assistant",
                    format!("tool_call {name} {}", serde_json::to_string(&args).unwrap_or_default()),
                ));
            }
            EventKind::ToolResult | EventKind::ToolFailed => {
                let text = e
                    .payload
                    .get("result")
                    .or_else(|| e.payload.get("error"))
                    .map(|v| serde_json::to_string(v).unwrap_or_default())
                    .unwrap_or_default();
                out.push(Message::new(
                    "tool",
                    format!(
                        "{}: {text}",
                        if e.kind == EventKind::ToolFailed { "failed" } else { "result" }
                    ),
                ));
            }
            _ => {}
        }
    }
    out
}

fn build_task_state(events: &[Event]) -> String {
    let mut turns = 0usize;
    let mut last_goal = String::new();
    let mut open_calls: Vec<String> = Vec::new();
    let mut last_failure = String::new();
    let mut cancelled = false;
    for e in events {
        match e.kind {
            EventKind::UserMessage => {
                turns += 1;
                last_goal = e
                    .payload
                    .get("text")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .chars()
                    .take(200)
                    .collect();
            }
            EventKind::ToolCall => {
                open_calls.push(
                    e.payload
                        .get("name")
                        .and_then(|x| x.as_str())
                        .unwrap_or("tool")
                        .to_string(),
                );
            }
            EventKind::ToolResult => {
                open_calls.pop();
            }
            EventKind::ToolFailed => {
                open_calls.pop();
                last_failure = e
                    .payload
                    .get("error")
                    .map(|v| v.to_string())
                    .unwrap_or_default()
                    .chars()
                    .take(120)
                    .collect();
            }
            EventKind::ToolCancelled => {
                cancelled = true;
                open_calls.clear();
            }
            _ => {}
        }
    }
    let mut s = format!("回合数: {turns}\n当前目标: {last_goal}\n");
    if !open_calls.is_empty() {
        s.push_str(&format!("未完成工具调用: {}\n", open_calls.join(", ")));
    }
    if !last_failure.is_empty() {
        s.push_str(&format!("最近工具失败: {last_failure}\n"));
    }
    if cancelled {
        s.push_str("上一轮被用户取消（不要重放已取消的工具）\n");
    }
    s
}

fn summarize_prefix(events: &[Event], max_chars: usize) -> (String, Vec<u64>) {
    let mut text = String::new();
    let mut refs = Vec::new();
    for e in events {
        if let Some(t) = e
            .payload
            .get("text")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
        {
            let tag = match e.kind {
                EventKind::UserMessage => "用户",
                EventKind::ModelMessage => "助手",
                _ => continue,
            };
            let short: String = t.chars().take(60).collect();
            let line = format!("{tag}: {short}\n");
            if text.len() + line.len() > max_chars {
                break;
            }
            text.push_str(&line);
            refs.push(e.seq);
        }
    }
    if !text.is_empty() {
        text.insert_str(0, "（更早对话摘要）\n");
    }
    (text, refs)
}

fn inject_skills(skills: &[Skill], query: &str) -> (String, usize, Vec<String>) {
    let mut out = String::new();
    let mut tokens = 0usize;
    let mut names = Vec::new();
    for s in skills {
        if !s.matches(query) {
            continue;
        }
        let text = s.compact();
        let t = estimate_tokens(&text);
        if tokens + t > SKILL_TOKENS_TOTAL {
            break;
        }
        tokens += t;
        out.push_str(&text);
        out.push('\n');
        names.push(s.name.clone());
    }
    (out, tokens, names)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub regex: Vec<String>,
    pub instructions: String,
    #[serde(default)]
    pub budget: usize,
}

impl Skill {
    pub fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return false;
        }
        if self.keywords.iter().any(|k| query.contains(k.as_str())) {
            return true;
        }
        self.regex.iter().any(|r| simple_match(r, query))
    }

    pub fn compact(&self) -> String {
        let budget = if self.budget == 0 { SKILL_TOKENS_EACH } else { self.budget.min(SKILL_TOKENS_EACH) };
        let mut s = format!("· {}：{}", self.name, self.instructions);
        if estimate_tokens(&s) > budget {
            let max_chars = budget.saturating_mul(2);
            s = format!("· {}：{}", self.name, self.instructions.chars().take(max_chars).collect::<String>());
        }
        s
    }

    /// 极简 `skill.yaml` 解析：`key: value`，`keywords: [a, b]`，正文在 `instructions: |` 之下。
    pub fn from_yaml(text: &str) -> Option<Skill> {
        let mut name = String::new();
        let mut keywords = Vec::new();
        let mut regex = Vec::new();
        let mut instructions = String::new();
        let mut budget = 0usize;
        let mut in_body = false;
        for raw in text.lines() {
            let line = raw.trim_end();
            if in_body {
                instructions.push_str(line.trim_start());
                instructions.push('\n');
                continue;
            }
            if line.trim() == "instructions: |" {
                in_body = true;
                continue;
            }
            if let Some((k, v)) = line.split_once(':') {
                let key = k.trim();
                let val = v.trim();
                match key {
                    "name" => name = val.trim_matches('"').to_string(),
                    "keywords" => keywords = parse_list(val),
                    "regex" => regex = parse_list(val),
                    "budget" => budget = val.parse().unwrap_or(0),
                    _ => {}
                }
            }
        }
        if name.is_empty() {
            return None;
        }
        Some(Skill {
            name,
            keywords,
            regex,
            instructions,
            budget,
        })
    }
}

fn parse_list(v: &str) -> Vec<String> {
    let v = v.trim().trim_start_matches('[').trim_end_matches(']');
    v.split(',')
        .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// 极简正则：只支持字面量子串与 `.*`（技能命中条件足够用）。
fn simple_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split(".*").collect();
    if parts.len() == 1 {
        return text.contains(pattern);
    }
    let mut pos = 0usize;
    for part in parts.iter() {
        if part.is_empty() {
            continue;
        }
        match text[pos..].find(part) {
            Some(idx) => pos += idx + part.len(),
            None => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(seq: u64, kind: EventKind, v: Value) -> Event {
        Event::new(seq, seq, kind, v)
    }

    #[test]
    fn assembles_with_stable_prefix_and_stats() {
        let engine = ContextEngine::default();
        let mut events = Vec::new();
        for i in 0..20u64 {
            events.push(ev(i * 2 + 1, EventKind::UserMessage, json!({"text": format!("用户问题 {i} {}", "x".repeat(40))})));
            events.push(ev(i * 2 + 2, EventKind::ModelMessage, json!({"text": format!("回答 {i} {}", "y".repeat(40))})));
        }
        let profile = ModelProfile {
            context_window: 900,
            max_output: 128,
            ..Default::default()
        };
        let a = engine
            .assemble(&events, &profile, &[json!({"name":"tool.fs.read"})], &[], &[])
            .unwrap();
        assert_eq!(a.messages[0].role, "system");
        assert!(a.messages[1].content.starts_with("[task_state]"));
        assert!(a.stats.total_tokens <= profile.context_window);
        assert!(a.stats.layers.l2_summary_cards >= 1, "older turns should be carded");
        assert!(a.stats.history_tokens > 0);
        assert!(a.stats.layers.l1_window_messages > 0);
    }

    #[test]
    fn rejects_more_than_12_tools() {
        let engine = ContextEngine::default();
        let tools: Vec<Value> = (0..13).map(|i| json!({"name": format!("t{i}")})).collect();
        let err = engine.assemble(&[], &ModelProfile::default(), &tools, &[], &[]);
        assert!(matches!(err, Err(CtxError::TooManyTools(13))));
    }

    #[test]
    fn skills_budget_and_match() {
        let long = "很长的指令".repeat(500);
        let s = Skill {
            name: "pdf".into(),
            keywords: vec!["PDF".into(), "pdf".into()],
            regex: vec![],
            instructions: long,
            budget: 0,
        };
        assert!(s.matches("帮我处理这个 PDF"));
        assert!(s.compact().chars().count() <= SKILL_TOKENS_EACH * 2 + 8);
        assert!(!Skill::from_yaml("name: x\nkeywords: [a, b]\ninstructions: |\n  做某事\nbudget: 20").unwrap().instructions.is_empty());
    }

    #[test]
    fn task_state_tracks_failure_and_cancel() {
        let events = vec![
            ev(1, EventKind::UserMessage, json!({"text":"跑一下测试"})),
            ev(2, EventKind::ToolCall, json!({"name":"tool.shell"})),
            ev(3, EventKind::ToolFailed, json!({"error":"exit 1"})),
            ev(4, EventKind::ToolCancelled, json!({})),
        ];
        let s = build_task_state(&events);
        assert!(s.contains("回合数: 1"));
        assert!(s.contains("最近工具失败"));
        assert!(s.contains("取消"));
    }

    #[test]
    fn token_estimate_handles_cjk_and_ascii() {
        assert!(estimate_tokens("你好世界") >= 4);
        assert!(estimate_tokens("hello world") < 6);
        assert_eq!(estimate_tokens(""), 1);
    }
}
