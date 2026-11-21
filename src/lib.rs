//! FeatherAgent 内核库。
//!
//! 分层对应设计文档 §2.1：契约层（`proto`）、编排核心（`engine`、`bus`）、
//! 事件日志（`log`）、插件运行时（`runtime`）、治理层（`governance`）、
//! 上下文引擎（`context`）、前端契约服务（`server`）。
//!
//! 三条不变式在代码里体现为：
//! 1. `engine` 不认识任何具体能力，只认识契约与事件；
//! 2. 所有能力经 `runtime` 的插件契约调用，内核内不直接实现 provider/tool；
//! 3. 一切状态变更都先落 `log`，`bus` 只做投影。

pub mod bus;
pub mod context;
pub mod engine;
pub mod governance;
pub mod log;
pub mod net;
pub mod proto;
pub mod runtime;
pub mod server;
pub mod util;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
