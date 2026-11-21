//! 插件运行时（设计文档 §3.3 / §4）。
//!
//! 形态：
//! - **进程外**（主推）：独立进程 = 唯一真安全边界；stdio 长度前缀帧 + JSON-RPC；
//! - **WASM（wasmi）**：进程内沙箱，fuel 计量，无系统调用（见 `wasm`）；
//! - 内置：仅契约适配与日志写入，不在这里。
//!
//! 崩溃语义（§4.5）：插件崩溃 → 工具重试 1 次 → 仍失败则降级续跑；
//! 连续 3 次熔断该插件（`plugin.degraded`）。

pub mod plugin;
pub mod registry;
pub mod wasm;

pub use plugin::{PluginError, PluginHost, PluginProc, PluginSpec, PluginState};
pub use registry::Registry;
