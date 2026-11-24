//! 事件类型与契约数据结构。
//!
//! 事件 kind 是 u16 枚举（写进日志记录的 `+4` 字段），与字符串名一一对应。
//! 与设计文档 §3.1 的记录布局严格对齐。

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const CONTRACT_API: &str = "1.0";

/// 事件类型枚举。码值写进日志，**一旦发布不可复用**。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    UserMessage,
    ModelDelta,
    ModelMessage,
    ToolCall,
    ToolResult,
    ToolFailed,
    ToolError,
    ToolDegraded,
    ToolCancelled,
    SessionCreated,
    SessionForked,
    SessionArchived,
    SessionResumed,
    LogRecovered,
    SubscriberLagged,
    PluginDegraded,
    PermissionDenied,
    CtxSnapshot,
    SkillInjected,
    MemoryRecalled,
    ConfigChanged,
    ChatFinished,
    ModelRetry,
    ContextCompacted,
    ReviewGuide,
    ReviewVerdict,
    ReviewFinished,
    ReviewFeedback,
    ReviewWarn,
}

impl EventKind {
    pub fn code(self) -> u16 {
        use EventKind::*;
        match self {
            UserMessage => 1,
            ModelDelta => 2,
            ModelMessage => 3,
            ToolCall => 4,
            ToolResult => 5,
            ToolFailed => 6,
            ToolError => 22,
            ToolDegraded => 23,
            ToolCancelled => 7,
            SessionCreated => 8,
            SessionForked => 9,
            SessionArchived => 10,
            SessionResumed => 11,
            LogRecovered => 12,
            SubscriberLagged => 13,
            PluginDegraded => 14,
            PermissionDenied => 15,
            CtxSnapshot => 16,
            SkillInjected => 17,
            MemoryRecalled => 18,
            ConfigChanged => 19,
            ChatFinished => 20,
            ModelRetry => 21,
            ContextCompacted => 24,
            ReviewGuide => 25,
            ReviewVerdict => 26,
            ReviewFinished => 27,
            ReviewFeedback => 28,
            ReviewWarn => 29,
        }
    }

    pub fn from_code(code: u16) -> Option<Self> {
        use EventKind::*;
        Some(match code {
            1 => UserMessage,
            2 => ModelDelta,
            3 => ModelMessage,
            4 => ToolCall,
            5 => ToolResult,
            6 => ToolFailed,
            7 => ToolCancelled,
            22 => ToolError,
            23 => ToolDegraded,
            8 => SessionCreated,
            9 => SessionForked,
            10 => SessionArchived,
            11 => SessionResumed,
            12 => LogRecovered,
            13 => SubscriberLagged,
            14 => PluginDegraded,
            15 => PermissionDenied,
            16 => CtxSnapshot,
            17 => SkillInjected,
            18 => MemoryRecalled,
            19 => ConfigChanged,
            20 => ChatFinished,
            21 => ModelRetry,
            24 => ContextCompacted,
            25 => ReviewGuide,
            26 => ReviewVerdict,
            27 => ReviewFinished,
            28 => ReviewFeedback,
            29 => ReviewWarn,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        use EventKind::*;
        match self {
            UserMessage => "user.message",
            ModelDelta => "model.delta",
            ModelMessage => "model.message",
            ToolCall => "tool.call",
            ToolResult => "tool.result",
            ToolFailed => "tool.failed",
            ToolError => "tool.error",
            ToolDegraded => "tool.degraded",
            ToolCancelled => "tool.cancelled",
            SessionCreated => "session.created",
            SessionForked => "session.forked",
            SessionArchived => "session.archived",
            SessionResumed => "session.resumed",
            LogRecovered => "log.recovered",
            SubscriberLagged => "subscriber.lagged",
            PluginDegraded => "plugin.degraded",
            PermissionDenied => "permission.denied",
            CtxSnapshot => "ctx.snapshot",
            SkillInjected => "skill.injected",
            MemoryRecalled => "memory.recalled",
            ConfigChanged => "config.changed",
            ChatFinished => "chat.finished",
            ModelRetry => "model.retry",
            ContextCompacted => "context.compacted",
            ReviewGuide => "review.guide",
            ReviewVerdict => "review.verdict",
            ReviewFinished => "review.finished",
            ReviewFeedback => "review.feedback",
            ReviewWarn => "review.warn",
        }
    }

    /// 从字符串名解析，未知返回 None。用于 `event.subscribe(kinds)` 与插件 manifest。
    pub fn parse(s: &str) -> Option<Self> {
        use EventKind::*;
        Some(match s {
            "user.message" => UserMessage,
            "model.delta" => ModelDelta,
            "model.message" => ModelMessage,
            "tool.call" => ToolCall,
            "tool.result" => ToolResult,
            "tool.failed" => ToolFailed,
            "tool.error" => ToolError,
            "tool.degraded" => ToolDegraded,
            "tool.cancelled" => ToolCancelled,
            "session.created" => SessionCreated,
            "session.forked" => SessionForked,
            "session.archived" => SessionArchived,
            "session.resumed" => SessionResumed,
            "log.recovered" => LogRecovered,
            "subscriber.lagged" => SubscriberLagged,
            "plugin.degraded" => PluginDegraded,
            "permission.denied" => PermissionDenied,
            "ctx.snapshot" => CtxSnapshot,
            "skill.injected" => SkillInjected,
            "memory.recalled" => MemoryRecalled,
            "config.changed" => ConfigChanged,
            "chat.finished" => ChatFinished,
            "model.retry" => ModelRetry,
            "context.compacted" => ContextCompacted,
            "review.guide" => ReviewGuide,
            "review.verdict" => ReviewVerdict,
            "review.finished" => ReviewFinished,
            "review.feedback" => ReviewFeedback,
            "review.warn" => ReviewWarn,
            _ => return None,
        })
    }

    pub const ALL: &'static [EventKind] = &[
        Self::UserMessage,
        Self::ModelDelta,
        Self::ModelMessage,
        Self::ToolCall,
        Self::ToolResult,
        Self::ToolFailed,
        Self::ToolError,
        Self::ToolDegraded,
        Self::ToolCancelled,
        Self::SessionCreated,
        Self::SessionForked,
        Self::SessionArchived,
        Self::SessionResumed,
        Self::LogRecovered,
        Self::SubscriberLagged,
        Self::PluginDegraded,
        Self::PermissionDenied,
        Self::CtxSnapshot,
        Self::SkillInjected,
        Self::MemoryRecalled,
        Self::ConfigChanged,
        Self::ChatFinished,
        Self::ModelRetry,
        Self::ContextCompacted,
        Self::ReviewGuide,
        Self::ReviewVerdict,
        Self::ReviewFinished,
        Self::ReviewFeedback,
    ];
}

/// 已解码的事件（读日志 / 总线分发共用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub seq: u64,
    /// 墙钟 `CLOCK_REALTIME`，Unix epoch 纳秒（§3.1 勘误 R1-1）。
    pub ts_wall_ns: u64,
    pub kind: EventKind,
    pub payload: Value,
}

impl Event {
    pub fn new(seq: u64, ts_wall_ns: u64, kind: EventKind, payload: Value) -> Self {
        Self {
            seq,
            ts_wall_ns,
            kind,
            payload,
        }
    }
}

/// 插件清单：握手时由插件上报，内核据此校验 API 版本区间与权限。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    /// 形如 `>=1.0 <2.0`
    pub api: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// 声明式权限，如 `fs:read`、`net:local`、`exec:shell`
    #[serde(default)]
    pub permissions: Vec<String>,
    /// `tool` / `provider` / `storage` / `context` / `bridge`
    #[serde(default)]
    pub category: String,
    /// 插件自述的设置项；内核只转述与校验，不解释语义（§4.6）。
    #[serde(default)]
    pub settings: Option<SettingsSchema>,
}

/// 插件声明的设置分组：由插件在 `plugin.json` 里自述，内核原样转述给任意前端。
///
/// 这是“装一个插件不要为它单独适配 UI”的关键：前端只认 `fields` 的类型与范围，
/// 不认识具体插件；GUI / TUI / WebUI 都按同一份声明渲染表单。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SettingsSchema {
    /// 设置在 config 中的根路径；空表示 `plugin_settings.<插件名>`。
    #[serde(default)]
    pub root: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub fields: Vec<SettingField>,
}

/// 单个设置项。`kind` 只用 `bool` / `int` / `number` / `string` / `enum` 五种，
/// 保证任何前端都能在不了解插件的情况下渲染。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingField {
    /// 相对 `root` 的键；支持点号表示嵌套（如 `guide.model`）。
    pub key: String,
    /// 绝对 config 路径；非空时覆盖 `root + key`（用于把插件设置指到既有顶层键）。
    #[serde(default)]
    pub path: String,
    pub label: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub default: Value,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub placeholder: String,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub step: Option<f64>,
    #[serde(default)]
    pub options: Vec<String>,
    /// 选项来源：非空时由前端向内核询问后填充（目前只有 `models`）。
    /// 内核只转述声明，不替前端解析列表，这样 GUI 插件也能复用同一份数据源。
    #[serde(default)]
    pub options_source: String,
    /// 是否允许填写列表外的值（模型名通常为 true：本地服务可能返回不了列表）。
    #[serde(default)]
    pub allow_custom: bool,
    #[serde(default)]
    pub secret: bool,
}

impl SettingsSchema {
    /// 设置落在 config 的根路径（点号分隔）。
    pub fn root_for(&self, plugin: &str) -> String {
        if self.root.trim().is_empty() {
            format!("plugin_settings.{plugin}")
        } else {
            self.root.trim().to_string()
        }
    }
}

impl SettingField {
    /// 该字段在 config 中的完整路径。
    pub fn path(&self, root: &str) -> String {
        if !self.path.trim().is_empty() {
            return self.path.trim().to_string();
        }
        if self.key.trim().is_empty() {
            root.to_string()
        } else {
            format!("{root}.{}", self.key.trim())
        }
    }

    /// 类型与区间校验；错误信息给前端直接显示。
    pub fn validate(&self, v: &Value) -> Result<(), String> {
        match self.kind.as_str() {
            "bool" => {
                if v.is_boolean() {
                    Ok(())
                } else {
                    Err(format!("{} 必须是布尔值", self.label))
                }
            }
            "int" => {
                let n = v
                    .as_i64()
                    .ok_or_else(|| format!("{} 必须是整数", self.label))?;
                if let Some(m) = self.min {
                    if (n as f64) < m {
                        return Err(format!("{} 不能小于 {}", self.label, m));
                    }
                }
                if let Some(m) = self.max {
                    if (n as f64) > m {
                        return Err(format!("{} 不能大于 {}", self.label, m));
                    }
                }
                Ok(())
            }
            "number" => {
                let n = v
                    .as_f64()
                    .ok_or_else(|| format!("{} 必须是数字", self.label))?;
                if !n.is_finite() {
                    return Err(format!("{} 必须是有限数字", self.label));
                }
                if let Some(m) = self.min {
                    if n < m {
                        return Err(format!("{} 不能小于 {}", self.label, m));
                    }
                }
                if let Some(m) = self.max {
                    if n > m {
                        return Err(format!("{} 不能大于 {}", self.label, m));
                    }
                }
                Ok(())
            }
            "string" => v
                .as_str()
                .map(|_| ())
                .ok_or_else(|| format!("{} 必须是字符串", self.label)),
            "enum" => {
                let s = v
                    .as_str()
                    .ok_or_else(|| format!("{} 必须是字符串", self.label))?;
                if self.options.is_empty() || self.options.iter().any(|o| o == s) {
                    Ok(())
                } else {
                    Err(format!(
                        "{} 只能是：{}",
                        self.label,
                        self.options.join(" / ")
                    ))
                }
            }
            other => Err(format!("{} 的类型 {other} 不受支持", self.label)),
        }
    }
}

/// 工具风险级别（§4.5 / §13 协作式权限门）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    Safe,
    Dangerous,
}

impl Risk {
    pub fn parse(s: &str) -> Risk {
        if s.eq_ignore_ascii_case("dangerous") {
            Risk::Dangerous
        } else {
            Risk::Safe
        }
    }
}

/// 会话角色：首个 attach 者为 holder（可写），其余为观察视图。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Holder,
    Observer,
}

pub fn now_wall_ns() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_error_and_degraded_codes_round_trip() {
        for kind in [
            EventKind::ToolError,
            EventKind::ToolDegraded,
            EventKind::ModelRetry,
            EventKind::ContextCompacted,
        ] {
            assert_eq!(EventKind::from_code(kind.code()), Some(kind));
            assert_eq!(EventKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(EventKind::ToolError.code(), 22);
        assert_eq!(EventKind::ToolDegraded.code(), 23);
        assert_eq!(EventKind::ContextCompacted.code(), 24);
        // rc.3.1：review.warn 是新增的软告警事件，编号必须紧接 review.feedback。
        assert_eq!(EventKind::ReviewWarn.code(), 29);
        assert_eq!(EventKind::from_code(29), Some(EventKind::ReviewWarn));
        assert_eq!(EventKind::ReviewWarn.as_str(), "review.warn");
        assert_eq!(EventKind::parse("review.warn"), Some(EventKind::ReviewWarn));
    }

    fn field(kind: &str, key: &str) -> SettingField {
        SettingField {
            key: key.into(),
            path: String::new(),
            label: key.into(),
            kind: kind.into(),
            default: serde_json::Value::Null,
            description: String::new(),
            placeholder: String::new(),
            min: Some(1.0),
            max: Some(5.0),
            step: Some(1.0),
            options: vec!["a".into(), "b".into()],
            options_source: String::new(),
            allow_custom: false,
            secret: false,
        }
    }

    #[test]
    fn setting_field_path_and_root_default() {
        let mut s = SettingsSchema::default();
        assert_eq!(s.root_for("review-gate"), "plugin_settings.review-gate");
        s.root = "review_gate".into();
        assert_eq!(s.root_for("review-gate"), "review_gate");
        assert_eq!(
            field("int", "max_rounds").path(&s.root_for("x")),
            "review_gate.max_rounds"
        );
        let mut f = field("int", "x");
        f.path = "search_count".into();
        assert_eq!(f.path("ignored"), "search_count", "绝对 path 覆盖 root");
    }

    #[test]
    fn setting_field_validates_type_and_range() {
        assert!(field("int", "n").validate(&serde_json::json!(3)).is_ok());
        assert!(field("int", "n").validate(&serde_json::json!(9)).is_err());
        assert!(field("int", "n").validate(&serde_json::json!("3")).is_err());
        assert!(field("number", "n")
            .validate(&serde_json::json!(2.5))
            .is_ok());
        assert!(field("bool", "n")
            .validate(&serde_json::json!(true))
            .is_ok());
        assert!(field("bool", "n")
            .validate(&serde_json::json!("true"))
            .is_err());
        assert!(field("enum", "n").validate(&serde_json::json!("a")).is_ok());
        assert!(field("enum", "n")
            .validate(&serde_json::json!("z"))
            .is_err());
        assert!(field("string", "n")
            .validate(&serde_json::json!("x"))
            .is_ok());
        assert!(field("string", "n")
            .validate(&serde_json::json!(1))
            .is_err());
    }
}
