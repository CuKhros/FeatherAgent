# FeatherAgent（轻羽）

面向 **1B~14B 本地小模型**的「上下文引擎」+ 一个**体积极小、零运行时依赖**的可移植 Agent 内核。

## 三个卖点

1. **小模型上下文工程** —— 技术差异所在，研发重心（四层压缩、预算器、容错解析、KV Cache 友好前缀）
2. **极致轻的内核** —— Rust 单二进制，目标 **≤ 8 MB**，空闲 RSS **≤ 15 MB**，冷启动 **< 20 ms**
3. **可移植性** —— 零运行时依赖（无 Node / 无 Python / 无 JVM）；安装根 / 数据目录 / 工作空间三类路径可独立外置

「一切皆插件」是实现手段，不作为卖点宣称。

## 架构骨架

三进程模型，**两条契约、一个内核**：

```
前端层（TUI 随主包 / GUI 独立包 / 第三方前端）
   │  ① 前端契约 Frontend Contract（IDL v1，UDS / 命名管道；WS 桥归 GUI 包）
内核（Rust 单二进制 ≤ 8 MB）：编排核心 · 追加日志 · 插件运行时 · 治理层 · 契约层
   │  ② 插件契约 Plugin Contract（IDL v1，stdio JSON-RPC / MCP 兼容）
插件进程层：context-engine · storage/index · provider-* · tool-* · bridge-*
   │
本地推理运行时（外部）：Ollama / llama.cpp / LM Studio / vLLM
```

四条内核不变式：内核不懂 Agent · 能力皆插件 · 一切可追溯（append-only 事件流）· **内核不持有业务存储**。

## 关键决策（v1.1）

| 项 | 决策 |
|---|---|
| 内核语言 | **Rust**（v1.0 的 Go 翻转为 Rust；Go 保留为可回退项，插件与前端零改动） |
| 事件日志 | **自研追加格式**（分段 + CRC32C + `seq` 排序，墙钟 `CLOCK_REALTIME` 落库），不含 SQLite |
| 索引 / 向量 | 下沉为 **独立 storage 插件进程**（默认 SQLite + FTS5，可选 redb / tantivy / 外部服务） |
| 插件沙箱 | 进程外 RPC（真安全边界）+ **WASM（wasmi）**；已移除 goja / JS 插件 |
| 前端 | TUI（随主包）+ GUI（**独立分发**，Web 资源 + `featheragent-gui` 桥接 + 浏览器 `--app` 宿主，原生壳可选） |
| 分发 | 三个产物：`featheragent-core` / `featheragent-gui` / `featheragent-plugins` |
| 路线图 | **阶段 + 门禁（Gate）**，不排工期：Phase 0 三个 Spike → 1 内核 → 2 上下文引擎 → 3 存储与安全 → 4 GUI → 5 分发 |

## 体积口径（三套指标，互不混用）

| 指标 | 目标 | 性质 |
|---|---|---|
| 开箱门禁 | 软 ≤ 100 MiB / 硬 ≤ 1 GiB | CI 硬断言 |
| 稳态占用 | 1 ~ 2.5 GiB | 监控指标 |
| 用户配额 | 默认 4 GiB | UI 可见 + 一键清理 |

## 非目标

不内置推理引擎、不分发权重、不做云账号、不遥测、不做移动端。

## 快速开始（Linux x64）

要求 Rust 1.80+，无其他运行时依赖。

```bash
cargo build --release --bins
./target/release/featheragent serve --root .          # 内核 + UDS
./target/release/featheragent-tui --root .            # TUI（另开终端）
./target/release/featheragent-gui --root . --app      # GUI（Chromium --app 独立窗口）
```

出厂插件在 `plugins/`（provider-mock / provider-ollama / tool / storage）；`serve` 扫描 `<安装根>/plugins`，默认 `provider-mock`。接真实 Ollama：先 `ollama serve`，再 `serve --provider provider-ollama --model qwen2.5:7b`。

三产物打包：`scripts/package.sh`，输出 `dist/*.tar.gz` + `sha256` + manifest；体积门禁（内核 ≤ 8 MiB）在脚本内断言。

运行时会在安装根下生成 `data/`、`workspace/`、`runtime/`；可用 `--data/--workspace/--runtime` 分别外置。

## 快速开始（Windows x64 测试包）

`FeatherAgent-1.1.3-windows-x64.zip` 解压到有写权限的目录（不要放 `Program Files`），然后：

```powershell
.\featheragent.exe help                       # 先看命令表
.\featheragent.exe serve --root .
.\featheragent-tui.exe --root .              # 另开终端
.\featheragent-gui.exe --root . --app          # 自动找 Chrome/Edge
.\featheragent.exe chat --root . --text "hello"   # 无 GUI/TUI 的脚本通道
```

5 个 exe 只依赖 Windows 系统 DLL（无 VC++ 运行库 / 无 MinGW 运行时）。构建：`scripts/build-windows.sh`（mingw-w64 + `x86_64-pc-windows-gnu` + 静态 CRT）；打包：`scripts/package-windows.sh`。详见 [Windows 测试说明](docs/Windows测试说明.md)。

## 文档

- **[总体设计 v1.1](docs/FeatherAgent架构设计文档-v1.1.md)** —— 全文（含 v1.0 → v1.1 修订说明、勘误 R1、技术清单、路线图与风险对策）
- **[验证报告](docs/验证报告.md)** —— 测试矩阵、实测门禁数字、已修缺陷、未测项与偏差
- **[前端契约 · Wire Format](docs/前端契约-WireFormat.md)** —— 4 字节小端长度 + MsgPack/JSON、端点发现、握手顺序、错误码
- **[v1.1.3 Windows 修复对照](docs/v1.1.3-Windows修复说明.md)** —— v1.1.2 复测 NEW-1～NEW-4 的根因、修法与验证状态
- **[v1.1.2 Windows 修复对照](docs/v1.1.2-Windows修复说明.md)** —— 上一轮 BUG-1～BUG-9 的修复记录

> 文档中所有「目标值」必须经 Spike / 阶段门禁实测后回填，未测不写数字。

## 状态

`v1.1.3` 是针对 v1.1.2 Windows 复测报告的修复版：原生/OpenAI `tool_calls` 结构化解析并接入工具执行；空响应不再静默 `status=ok`；插件子进程补齐 `NO_PROXY` 回环；bench 门禁按平台分档（Windows 启动 <100 ms、写入 P95 <10 ms）；客户端断开触发取消并释放会话 holder；静音管道收尾的 os error 232。Linux 自动化测试全绿。

已实测范围：Linux x64、mock provider 全链路、TUI（PTY 交互）、GUI（HTTP + WebSocket + SSE 回退，`--app` 独立 Profile 启动 Chromium）、三产物打包与解压运行、1000 次日志随机截断恢复。

Windows x64：v1.1.2 在真机复测中 8/9 个旧 BUG 已修复；新暴露的工具调用、空响应、平台门禁与断连取消在 v1.1.3 修复。v1.1.3 仍需真机复测；Wine 只能做基本冒烟，不能复现真实 Windows 的管道/ConPTY/代理环境。

未实测：真实 Ollama / 7B 模型指标（S3）、Windows 实机、macOS、四平台 × 双架构矩阵。详见验证报告。

