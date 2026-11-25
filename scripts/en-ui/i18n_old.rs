//! 轻量界面语言：英文为默认，`--lang zh-CN` 保留中文。
//!
//! 内核本身与语言无关；这里只服务 TUI 的界面文案。

use std::sync::atomic::{AtomicBool, Ordering};

static ZH: AtomicBool = AtomicBool::new(false);

/// 解析 `--lang`；默认英文。返回是否中文。
pub fn set_lang(tag: Option<&str>) -> bool {
    let zh = matches!(tag, Some("zh") | Some("zh-CN") | Some("zh_CN") | Some("cn"));
    ZH.store(zh, Ordering::Relaxed);
    zh
}

pub fn is_zh() -> bool {
    ZH.load(Ordering::Relaxed)
}

pub fn is_en() -> bool {
    !is_zh()
}

/// 静态二选一，避免在绘制代码里重复写 if。
pub fn t(en: &'static str, zh: &'static str) -> &'static str {
    if is_zh() {
        zh
    } else {
        en
    }
}

/// 将界面里的中文短语换成英文。中文模式原样返回。
/// 动态消息用短语替换，保留数字、路径、模型名等原值。
pub fn tr(text: &str) -> String {
    if is_zh() {
        return text.to_string();
    }
    let mut out = text.to_string();
    for (zh, en) in PHRASES {
        if out.contains(zh) {
            out = out.replace(zh, en);
        }
    }
    out
}

/// 长短语优先，避免短词先替换后破坏长句。
const PHRASES: &[(&str, &str)] = &[
    // 连接/会话/项目
    ("已恢复会话 ", "Restored session "),
    ("｜项目 ", " | project "),
    (" 条事件", " events"),
    ("恢复失败: ", "Restore failed: "),
    ("新会话 ", "New session "),
    ("创建失败: ", "Create failed: "),
    ("项目容器 ", "Project container "),
    ("读取项目失败", "Failed to read project"),
    // 插件
    ("启停失败: ", "Toggle failed: "),
    ("（已同步运行进程）", " (running process synced)"),
    ("（运行进程已同步）", " (running process synced)"),
    ("读取插件失败", "Failed to read plugins"),
    ("读取插件侧栏失败", "Failed to read plugin sidebar"),
    ("插件未安装", "plugin not installed"),
    // 设置
    ("读取配置失败: ", "Failed to read configuration: "),
    (
        "读取配置成功；←/↑↓ 侧栏，Tab 表单，Ctrl+S 保存",
        "Configuration loaded; ←/↑↓ sidebar, Tab form, Ctrl+S save",
    ),
    ("正在读取模型列表…", "Reading model list…"),
    (
        "联网：local · 本地公开网页 · 免费无需 Key",
        "Web: local · public web · free, no key",
    ),
    ("联网：local · 插件未就绪", "Web: local · plugin not ready"),
    ("联网：已关闭", "Web: off"),
    ("联网状态读取失败", "Failed to read web-search status"),
    (
        "模型列表不可用（可手动输入）",
        "Model list unavailable (manual entry allowed)",
    ),
    (
        " 个模型；←/→ 选择，或直接输入",
        " models; ←/→ select, or type directly",
    ),
    (
        "模型列表读取失败（可手动输入）",
        "Failed to read model list (manual entry allowed)",
    ),
    (
        "温度/上下文/回合数必须是数字",
        "Temperature/context/turns must be numbers",
    ),
    (
        "Base URL 和模型名不能为空",
        "Base URL and model name must not be empty",
    ),
    (
        "温度必须在 0.00–2.00 之间",
        "Temperature must be between 0.00 and 2.00",
    ),
    ("已保存并应用：", "Saved and applied: "),
    (
        "；工作区改动需重启 serve 生效",
        "; workspace changes apply after restarting serve",
    ),
    ("保存失败: ", "Save failed: "),
    (
        "已关闭设置页（未保存的修改已丢弃）",
        "Settings closed (unsaved changes discarded)",
    ),
    // 对话中的系统/工具消息
    ("请求 ", "Request "),
    (" 业务错误: ", " business error: "),
    (" 插件失败: ", " plugin failure: "),
    ("工具已熔断: ", "Tool circuit-breaker: "),
    ("（连续 ", " ("),
    (" 次技术失败）", " technical failures)"),
    ("拒绝: ", "Denied: "),
    ("内核配置已更新", "Kernel configuration updated"),
    ("上下文已压缩：", "Context compacted: "),
    (" 个旧事件 → ", " old events → "),
    (
        " 字符摘要（原文仍在日志）",
        " chars summary (original text remains in the log)",
    ),
    (
        "插件降级（连续失败 3 次）",
        "Plugin degraded (3 consecutive failures)",
    ),
    (
        "事件订阅落后，已丢弃旧事件",
        "Event subscription lagged; old events were dropped",
    ),
    ("— 本轮结束（", "— Turn complete ("),
    ("完成（", "Done ("),
    ("召回 ", "Recalled "),
    (" 条相关记忆", " relevant memories"),
    ("已连接 ", "Connected to "),
    ("；↑↓ 选会话，Enter 进入", "; ↑↓ select session, Enter open"),
    ("会话 ", "Session "),
    (
        "：↑↓ 翻阅对话，Esc 返回列表，/help 查看命令",
        ": ↑↓ scroll, Esc back, /help for commands",
    ),
    ("：↑↓ 翻阅对话，Esc 返回列表", ": ↑↓ scroll, Esc back"),
    (
        " 个插件；Space 启停，r 刷新，Esc 返回",
        " plugins; Space toggles, r refreshes, Esc back",
    ),
    ("已关闭插件管理", "Plugin management closed"),
    ("读取插件失败", "Failed to read plugins"),
    ("读取插件侧栏失败", "Failed to read plugin sidebar"),
    ("插件未安装", "plugin not installed"),
    ("读取配置失败", "Failed to read configuration"),
    (
        "读取配置成功；←/↑↓ 侧栏，Tab 表单，Ctrl+S 保存",
        "Configuration loaded; ←/↑↓ sidebar, Tab form, Ctrl+S save",
    ),
    ("正在读取模型列表…", "Reading model list…"),
    (
        "联网：local · 本地公开网页 · 免费无需 Key",
        "Web: local · public web · free, no key",
    ),
    ("联网：local · 插件未就绪", "Web: local · plugin not ready"),
    ("联网：已关闭", "Web: off"),
    ("联网：", "Web: "),
    ("已配置", "configured"),
    ("未配置", "not configured"),
    ("联网状态读取失败", "Failed to read web-search status"),
    (
        "模型列表不可用（可手动输入）",
        "Model list unavailable (manual entry allowed)",
    ),
    (
        " 个模型；←/→ 选择，或直接输入",
        " models; ←/→ select, or type directly",
    ),
    (
        "模型列表读取失败（可手动输入）",
        "Failed to read model list (manual entry allowed)",
    ),
    (
        "温度/上下文/回合数必须是数字",
        "Temperature/context/turns must be numbers",
    ),
    (
        "Base URL 和模型名不能为空",
        "Base URL and model name must not be empty",
    ),
    (
        "温度必须在 0.00–2.00 之间",
        "Temperature must be between 0.00 and 2.00",
    ),
    ("已保存并应用：", "Saved and applied: "),
    (
        "；工作区改动需重启 serve 生效",
        "; workspace changes apply after restarting serve",
    ),
    ("保存失败", "Save failed"),
    (
        "已关闭设置页（未保存的修改已丢弃）",
        "Settings closed (unsaved changes discarded)",
    ),
    ("已允许 ", "Allowed "),
    ("已拒绝 ", "Denied "),
    ("待确认（", "Pending confirmation ("),
    ("）: ", "): "),
    (" — y 允许 / n 拒绝", " — y allow / n deny"),
    (
        "会话列表：↑↓ 选择，Enter 进入，Esc 退出",
        "Sessions: ↑↓ select, Enter open, Esc quit",
    ),
    (
        "先按 Enter 进入会话，再输入消息",
        "Press Enter to open a session first",
    ),
    ("用法: /use <session>", "Usage: /use <session>"),
    (
        "用法: /workspace <项目名>；项目位于 workspace_root 下一层",
        "Usage: /workspace <project>; projects live one level under workspace_root",
    ),
    ("已创建项目 ", "Created project "),
    ("）；/new <会话> ", "); /new <session> "),
    (" 可绑定", " can bind"),
    ("创建项目失败", "Project creation failed"),
    ("项目容器 ", "Project container "),
    ("读取项目失败", "Failed to read project"),
    ("先 /new 或 /use", "Use /new or /use first"),
    ("已 fork → ", "Forked → "),
    ("（复制 {} 条）", " (copied {} events)"),
    ("fork 失败", "Fork failed"),
    (
        "已紧急停止；已生成内容与上下文已保存在会话日志",
        "Emergency stop complete; generated content and context are saved in the session log",
    ),
    ("停止失败", "Stop failed"),
    (
        "用法: /steer <插入给模型的引导>",
        "Usage: /steer <guidance for the model>",
    ),
    ("你（引导）", "You (steer)"),
    (
        "引导已写入上下文；当前生成会在边界中断并重新开始",
        "Steer written to context; the current generation stops at a boundary and restarts",
    ),
    ("插入引导失败", "Failed to insert steer"),
    ("已压缩上下文：", "Context compacted: "),
    (" 事件 → 摘要 ", " events → summary "),
    (" 字符（覆盖至 #", " chars (covers through #"),
    (
        "上下文压缩完成；旧原文仍保留在会话日志",
        "Compaction complete; original text remains in the session log",
    ),
    ("压缩失败", "Compaction failed"),
    (
        "插件管理：Space 启停，r 刷新，Esc 返回",
        "Plugin management: Space toggles, r refreshes, Esc back",
    ),
    (
        "运行中直接输入文本=插入引导；/stop=紧急停止并保存上下文",
        "Typing during a run = steer; /stop = emergency stop and save context",
    ),
    ("未知命令 /", "Unknown command /"),
    (
        "先 /new 或 /use 选择会话",
        "Use /new or /use to select a session",
    ),
    ("你（引导）", "You (steer)"),
    (
        "引导已插入上下文；当前生成中断后继续",
        "Steer inserted; the current generation continues after interruption",
    ),
    ("发送中…（", "Sending… ("),
    ("发送失败", "Send failed"),
    ("助手", "Assistant"),
    ("工具", "Tool"),
    ("系统", "System"),
    ("你", "You"),
    ("会话", "Sessions"),
    ("对话", "Conversation"),
    ("状态", "Status"),
    ("输入", "Input"),
    ("上下文", "Context"),
    ("预算 ", "Budget "),
    ("分层 L1 ", "Layers L1 "),
    (" 条 / L2 ", " items / L2 "),
    (" 卡 / L3 ", " cards / L3 "),
    (" tok / L4 ", " tok / L4 "),
    (" 项", " items"),
    ("模型 ", "Model "),
    (" · 事件 ", " · events "),
    ("目标: ", "Goal: "),
    ("摘要卡 ", "Summary cards "),
    (" 张", " cards"),
    (
        "（尚无快照，发送一条消息后出现）",
        "(No snapshot yet; send a message first)",
    ),
    ("⚠ 待确认 [", "⚠ Pending confirmation ["),
    ("y 允许 / n 拒绝", "y allow / n deny"),
    (
        "输入（Enter 发送，←/→ 移动光标，/help 命令）",
        "Input (Enter sends, ←/→ moves cursor, /help commands)",
    ),
    (
        "输入（会话列表：Enter 进入，↑↓ 选择，Esc 退出）",
        "Input (sessions: Enter open, ↑↓ select, Esc quit)",
    ),
    ("（", " ("),
    ("）", ")"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_context_placeholder_translates() {
        set_lang(None);
        assert_eq!(
            tr("（尚无快照，发送一条消息后出现）"),
            "(No snapshot yet; send a message first)"
        );
    }
}
