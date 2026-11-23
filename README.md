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
前端层（TUI 随主包 / WebUI 独立包 / 第三方前端）
   │  ① 前端契约 Frontend Contract（IDL v1，UDS / 命名管道；WS 桥归 WebUI 包）
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
| 前端 | TUI（随主包）+ **WebUI**（**独立分发**，Web 资源 + `featheragent-webui` 桥接 + 浏览器 `--app` 宿主；后续真 GUI 作为独立插件） |
| 分发 | 三个产物：`featheragent-core` / `featheragent-webui` / `featheragent-plugins` |
| 路线图 | **阶段 + 门禁（Gate）**，不排工期：Phase 0 三个 Spike → 1 内核 → 2 上下文引擎 → 3 存储与安全 → 4 WebUI → 5 分发 → 6 原生 GUI 插件 |

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
./target/release/featheragent-webui --root . --app    # WebUI（Chromium --app 独立窗口）
```

出厂插件在 `plugins/`（provider-mock / provider-ollama / tool / storage / search）；`serve` 扫描 `<安装根>/plugins`，默认 `provider-mock`。接真实 Ollama：先 `ollama serve`，再 `serve --provider provider-ollama --model qwen2.5:7b`。WebUI 右上角“设置”或 TUI `/settings` 可在线切换 provider、模型、温度、上下文参数与工作区；WebUI 顶栏“插件”和 TUI `/plugins` 提供插件安装/启停，设置左侧栏会为每个已登记插件生成页面。模型下拉显示 `provider.models` 返回的全部模型，保存后经 `config.set` 即时应用。`serve` 启动会立即打印 banner 并写 `runtime/ipc/starting` / `ready`，首次启动较慢时 `chat` 会明确提示“内核未就绪（启动中）”。

三产物打包：`scripts/package.sh`，输出 `dist/*.tar.gz` + `sha256` + manifest；体积门禁（内核 ≤ 8 MiB）在脚本内断言。

运行时会在安装根下生成 `data/`、`workspace/`、`runtime/`；可用 `--data/--workspace/--runtime` 分别外置，或在设置页把 `workspace_root` 指向安装目录外（保存后重启 `serve` 生效）。`workspace_root` 是**项目容器**：容器下每个项目目录才是工作区，各自拥有 `.featheagent/`（`worklog.md` / `memory.md` / `README.md`）；worklog 由内核在回合结束时自动追加真实 UTC 时间、目标与工具，模型不手写时间。运行中可 `/stop` 紧急停止并保留上下文，直接输入文本会作为 `chat.inject` 引导当前回合，`/compact` 手动压缩旧上下文。TUI 进入会话后 ↑↓ 翻阅对话、Esc 返回会话列表；←/→/Home/End 可编辑输入，CJK 光标按显示宽度对齐。联网搜索默认走内置 `search` 插件（`search_provider=local`），无需 API Key；设置页可切到 DeepSeek 或关闭。

## 快速开始（Windows x64 测试包）

`FeatherAgent-1.1.9-windows-x64.zip` 解压到有写权限的目录（不要放 `Program Files`），然后：

```powershell
.\featheragent.exe help                       # 先看命令表
.\featheragent.exe serve --root .
.\featheragent-tui.exe --root .              # 另开终端
.\featheragent-webui.exe --root . --app        # 自动找 Chrome/Edge
.\featheragent.exe chat --root . --text "hello"   # 无 WebUI/TUI 的脚本通道
```

5 个 exe 只依赖 Windows 系统 DLL（无 VC++ 运行库 / 无 MinGW 运行时）。`serve` 启动会立即打印 banner；首次启动可能 1–2 分钟（杀毒软件扫描插件二进制），期间会打印 `[init]` 进度；`runtime\ipc\ready` 未出现时 `chat` 会提示“内核未就绪（启动中）”，不是通道故障。构建：`scripts/build-windows.sh`（mingw-w64 + `x86_64-pc-windows-gnu` + 静态 CRT）；打包：`scripts/package-windows.sh`。详见 [Windows 测试说明](docs/Windows测试说明.md)。

## 平台门禁差异（v1.1.9）

| 门禁 | Linux | Windows | 说明 |
|---|---|---|---|
| 进程内启动 P50 | < 20 ms | **< 100 ms** | Windows 冷启动含更多系统调用开销 |
| 事件写入 P95 | < 100 µs | **< 10 ms** | NTFS `FlushFileBuffers` + 实时防护导致一个数量级差异，属环境特性 |
| 内核体积 | ≤ 8 MB | ≤ 8 MB | 共用 |
| 空闲 RSS | ≤ 15 MB | ≤ 15 MB | 共用 |
| 上下文组装 P95 | < 30 ms | < 30 ms | 共用 |
| RPC P95 | < 1 ms | < 1 ms | 共用 |

`bench --json` 会返回 `platform`、`boot_limit_ms`、`write_limit_us`，不会把 Windows 固定标红。

## 文档

- **[总体设计 v1.1](docs/FeatherAgent架构设计文档-v1.1.md)** —— 全文（含 v1.0 → v1.1 修订说明、勘误 R1、技术清单、路线图与风险对策）
- **[验证报告](docs/验证报告.md)** —— 测试矩阵、实测门禁数字、已修缺陷、未测项与偏差
- **[前端契约 · Wire Format](docs/前端契约-WireFormat.md)** —— 4 字节小端长度 + MsgPack/JSON、端点发现、握手顺序、错误码
- **[v1.1.9 Windows 修复对照](docs/v1.1.9-Windows修复说明.md)** —— P1 路径双重嵌套、`/stop` 状态、TUI/WebUI Markdown、羽毛图标、零 Key 本地联网搜索
- **[v1.1.8 Windows 修复对照](docs/v1.1.8-Windows修复说明.md)** —— 项目级工作区与每项目 `.featheagent`、TUI 设置侧栏、紧急停止/引导/压缩、联网搜索，以及 v1.1.7 复测 N1/N2/N3/N4/N6/N7
- **[v1.1.7 Windows 修复对照](docs/v1.1.7-Windows修复说明.md)** —— v1.1.6 复测 NEW-K、TUI 双模式/光标、外置工作区与 `.featheagent`、插件管理与设置侧栏
- **[v1.1.6 Windows 修复对照](docs/v1.1.6-Windows修复说明.md)** —— v1.1.5 复测 NEW-H～NEW-J、启动就绪标记、配置单字段原子更新、模型下拉全量展示
- **[v1.1.5 Windows 修复对照](docs/v1.1.5-Windows修复说明.md)** —— v1.1.4 复测 NEW-E～NEW-G、工具熔断粒度与 WebUI/TUI 设置页
- **[v1.1.4 Windows 修复对照](docs/v1.1.4-Windows修复说明.md)** —— v1.1.3 复测 NEW-A～NEW-D 与 WebUI 更名
- **[v1.1.3 Windows 修复对照](docs/v1.1.3-Windows修复说明.md)** —— 上一轮 NEW-1～NEW-4 的修复记录
- **[v1.1.2 Windows 修复对照](docs/v1.1.2-Windows修复说明.md)** —— 上一轮 BUG-1～BUG-9 的修复记录

> 文档中所有「目标值」必须经 Spike / 阶段门禁实测后回填，未测不写数字。

## 状态

`v1.1.9` 是基于 v1.1.8 Windows 真机复测报告的产品能力版：修复 P1 项目工作区路径双重嵌套、TUI 提前 `/stop` 误报 failed；TUI/WebUI 助手消息支持 Markdown；WebUI/任务栏/程序图标接入用户提供的羽毛图标；内置零 Key 本地联网搜索（必应/DuckDuckGo/Wikipedia/Hacker News/GitHub + 新闻/抓取/联想），默认 `search_provider=local`，DeepSeek 保留为可选计费后端。Linux 自动化测试 **85/85** 通过（56 lib + 2 plugin + 2 kernel + 3 TUI + 3 WebUI + 2 golden + 17 integration；另有 1 个需公网的集成用例单独跑通）。

已实测范围：Linux x64、mock provider 全链路、TUI（PTY 交互）、WebUI（HTTP + WebSocket + SSE 回退，`--app` 独立 Profile 启动 Chromium）、多模型下拉与保存、三产物打包与解压运行、1000 次日志随机截断恢复。

Windows x64：v1.1.3 真机确认 NEW-1～NEW-3；v1.1.4 确认 NEW-A～NEW-D；v1.1.5 确认 NEW-E～NEW-G，并暴露 NEW-H～NEW-J；v1.1.6 确认 NEW-H/I/J 与 NEW-4，仅余 NEW-K；v1.1.7 已发布并进入复测；v1.1.8 已发布并完成 P1/P2/P3/P4 与 §6.1 修复。v1.1.9 包 `FeatherAgent-1.1.9-windows-x64.zip` 的字节数与哈希以 Release 资产和 manifest 为准；仍需真机复测项目级路径、TUI/WebUI Markdown 与图标、本地联网搜索、`/stop` 状态及高 DPI/中文渲染。Wine 6.0.3 只作启动冒烟，boot/RSS 受 Wine 开销影响不作 Windows 结论。

未实测：Windows 真机项目级工作区、TUI 设置侧栏、停止/引导/压缩、联网搜索、高 DPI/中文渲染；DeepSeek Search API 真实调用（需 `DEEPSEEK_API_KEY`，按成功请求计费）；真实 Ollama / 7B 模型指标（S3 三档）、网络盘/同步盘、24h 浸泡、macOS、四平台 × 双架构矩阵。详见验证报告。

