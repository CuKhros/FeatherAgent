//! JSON-RPC 2.0 信封（契约传输层）。
//!
//! 只实现项目需要的最小子集：request / notification / success / error。

use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct Request {
    pub id: Option<u64>,
    pub method: String,
    pub params: Value,
}

impl Request {
    pub fn parse(v: &Value) -> Option<Request> {
        let method = v.get("method")?.as_str()?.to_string();
        let id = v.get("id").and_then(|x| x.as_u64());
        let params = v.get("params").cloned().unwrap_or(Value::Null);
        Some(Request { id, method, params })
    }

    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

pub fn success(id: u64, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

pub fn error(id: Option<u64>, code: i64, message: impl Into<String>) -> Value {
    let msg: String = message.into();
    match id {
        Some(id) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": msg}}),
        None => json!({"jsonrpc": "2.0", "error": {"code": code, "message": msg}}),
    }
}

pub fn notification(method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "method": method, "params": params})
}

pub fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

// 契约错误码（插件契约与前端契约共用一套编号，便于日志排障）
pub const ERR_METHOD_NOT_FOUND: i64 = -32601;
pub const ERR_INVALID_PARAMS: i64 = -32602;
pub const ERR_INTERNAL: i64 = -32603;
pub const ERR_OBSERVER_READONLY: i64 = 40301;
pub const ERR_NO_SESSION: i64 = 40401;
pub const ERR_BUDGET: i64 = 42901;
pub const ERR_PLUGIN: i64 = 50201;
pub const ERR_OCCUPIED: i64 = 40901;
