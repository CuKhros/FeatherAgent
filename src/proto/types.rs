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
