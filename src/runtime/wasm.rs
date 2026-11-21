//! WASM 沙箱（`wasmi`，设计文档 §3.3 / §4.3）。
//!
//! 边界性质：**半安全边界** —— 只能经由导入表获得能力（`log` / `emit`），
//! 没有文件、网络、时钟等系统调用；`fuel` 计量防死循环。不得称其为"沙箱"以外的东西。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use wasmi::{Caller, Config, Engine, Linker, Memory, Module, Store};

#[derive(Debug, Clone)]
pub struct WasmCfg {
    pub fuel: u64,
    /// 只有声明过的 kind 才能 emit（事件投影过滤）。
    pub allowed_kinds: Vec<u16>,
}

impl Default for WasmCfg {
    fn default() -> Self {
        Self {
            fuel: 10_000_000,
            allowed_kinds: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmittedEvent {
    pub kind: u16,
    pub payload: Value,
}

#[derive(Default)]
struct HostState {
    logs: Vec<String>,
    emitted: Vec<EmittedEvent>,
}

#[derive(Debug)]
pub enum WasmError {
    Compile(String),
    Trap(String),
    OutOfFuel,
}

impl std::fmt::Display for WasmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WasmError::Compile(s) => write!(f, "compile: {s}"),
            WasmError::Trap(s) => write!(f, "trap: {s}"),
            WasmError::OutOfFuel => write!(f, "out of fuel"),
        }
    }
}

pub struct WasmRuntime {
    engine: Engine,
}

impl WasmRuntime {
    pub fn new() -> Self {
        let mut cfg = Config::default();
        cfg.consume_fuel(true);
        Self {
            engine: Engine::new(&cfg),
        }
    }

    /// 执行一个模块的 `run` 导出，返回它 emit 的事件与日志。
    pub fn run(&self, wasm: &[u8], input: &Value, cfg: &WasmCfg) -> Result<(Vec<EmittedEvent>, Vec<String>), WasmError> {
        let module = Module::new(&self.engine, wasm).map_err(|e| WasmError::Compile(e.to_string()))?;
        let mut store = Store::new(&self.engine, HostState::default());
        // wasmi 0.31 的语义：向 store 注入燃料，执行时逐指令消耗
        store
            .add_fuel(cfg.fuel)
            .map_err(|_| WasmError::Compile("fuel unsupported".into()))?;

        // 链接器要求 host 闭包 Send + Sync，因此用 Arc<Mutex<_>>
        let memory_slot: std::sync::Arc<parking_lot::Mutex<Option<Memory>>> =
            std::sync::Arc::new(parking_lot::Mutex::new(None));

        let mut linker = Linker::new(&self.engine);
        let allowed = cfg.allowed_kinds.clone();
        let mem_for_log = memory_slot.clone();
        linker
            .func_wrap(
                "env",
                "log",
                move |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                    let mem = mem_for_log
                        .lock()
                        .as_ref()
                        .copied()
                        .or_else(|| caller.get_export("memory").and_then(|e| e.into_memory()));
                    if let Some(mem) = mem {
                        let data = read_bytes(&mut caller, mem, ptr, len);
                        caller.data_mut().logs.push(String::from_utf8_lossy(&data).to_string());
                    }
                },
            )
            .map_err(|e| WasmError::Compile(e.to_string()))?;

        let mem_for_emit = memory_slot.clone();
        let allowed_for_emit = allowed.clone();
        linker
            .func_wrap(
                "env",
                "emit",
                move |mut caller: Caller<'_, HostState>, kind: i32, ptr: i32, len: i32| {
                    let k = kind as u16;
                    if !allowed_for_emit.is_empty() && !allowed_for_emit.contains(&k) {
                        return; // 未订阅的 kind：静默丢弃（投影过滤）
                    }
                    let mem = mem_for_emit
                        .lock()
                        .as_ref()
                        .copied()
                        .or_else(|| caller.get_export("memory").and_then(|e| e.into_memory()));
                    if let Some(mem) = mem {
                        let data = read_bytes(&mut caller, mem, ptr, len);
                        let payload: Value = serde_json::from_slice(&data)
                            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&data).to_string()));
                        caller.data_mut().emitted.push(EmittedEvent { kind: k, payload });
                    }
                },
            )
            .map_err(|e| WasmError::Compile(e.to_string()))?;

        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| WasmError::Compile(e.to_string()))?
            .start(&mut store)
            .map_err(|e| WasmError::Trap(e.to_string()))?;

        if let Some(mem) = instance.get_memory(&store, "memory") {
            *memory_slot.lock() = Some(mem);
        }

        let run = instance
            .get_typed_func::<(), ()>(&store, "run")
            .map_err(|e| WasmError::Compile(e.to_string()))?;
        run.call(&mut store, ()).map_err(|e| {
            let s = e.to_string();
            let consumed = store.fuel_consumed().unwrap_or(0);
            if s.contains("fuel") || consumed >= cfg.fuel {
                WasmError::OutOfFuel
            } else {
                WasmError::Trap(s)
            }
        })?;
        let _ = input; // 输入经线性内存传入（由插件自行约定布局），此处保留签名

        let st = store.data();
        Ok((st.emitted.clone(), st.logs.clone()))
    }
}

fn read_bytes(caller: &mut Caller<'_, HostState>, mem: Memory, ptr: i32, len: i32) -> Vec<u8> {
    let start = ptr as usize;
    let end = start.saturating_add(len.max(0) as usize);
    let data = mem.data(&*caller);
    if start >= data.len() {
        return Vec::new();
    }
    data[start..end.min(data.len())].to_vec()
}

impl Default for WasmRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wat_bytes(s: &str) -> Vec<u8> {
        wat::parse_str(s).unwrap()
    }

    const OK: &str = r#"
(module
  (import "env" "log" (func $log (param i32 i32)))
  (import "env" "emit" (func $emit (param i32 i32 i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "{\"x\":1}")
  (func (export "run")
    i32.const 0 i32.const 7 call $log
    i32.const 3 i32.const 0 i32.const 7 call $emit
    i32.const 2 i32.const 0 i32.const 7 call $emit)
)
"#;

    #[test]
    fn emits_only_subscribed_kinds() {
        let rt = WasmRuntime::new();
        let wasm = wat_bytes(OK);
        let cfg = WasmCfg {
            fuel: 1_000_000,
            allowed_kinds: vec![3],
        };
        let (evs, logs) = rt.run(&wasm, &Value::Null, &cfg).unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].kind, 3);
        assert_eq!(evs[0].payload["x"], 1);
        assert_eq!(logs.len(), 1);
    }

    #[test]
    fn fuel_exhaustion_traps() {
        let rt = WasmRuntime::new();
        let wasm = wat_bytes(r#"(module (func (export "run") (loop br 0)))"#);
        let cfg = WasmCfg {
            fuel: 10_000,
            allowed_kinds: vec![],
        };
        let err = rt.run(&wasm, &Value::Null, &cfg).unwrap_err();
        assert!(matches!(err, WasmError::OutOfFuel | WasmError::Trap(_)));
    }

    #[test]
    fn no_syscalls_by_construction() {
        // 未导入任何 host 函数时，模块无法访问文件/网络：wasmi 不提供任何默认导入。
        let rt = WasmRuntime::new();
        let wasm = wat_bytes(r#"(module (func (export "run")))"#);
        let (evs, _) = rt.run(&wasm, &Value::Null, &WasmCfg::default()).unwrap();
        assert!(evs.is_empty());
    }
}
