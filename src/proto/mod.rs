//! 契约层：事件类型、插件清单、长度前缀帧、JSON-RPC 信封。
//!
//! 设计文档 §4.1：契约（IDL）与传输分离。本模块只定义字节与语义，
//! 不关心是 stdio、UDS 还是命名管道。

pub mod frame;
pub mod rpc;
pub mod types;

pub use frame::{read_frame, write_frame, Codec};
pub use types::{Event, EventKind, PluginManifest, CONTRACT_API};
