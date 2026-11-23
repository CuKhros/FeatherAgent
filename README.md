# FeatherAgent（轻羽）

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

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

## 平台支持矩阵

> 这是**当前验证状态**，不是对未来平台的承诺。未实测的平台不进入发布门禁。

| 平台 | 架构 | 构建方式 | 发布形态 | 当前状态 |
|---|---|---|---|---|
| Linux x64 | x86_64 | 原生 `cargo` / `scripts/build-linux.sh` | 三产物 tar.gz | **已实测**（自动化测试、TUI PTY、WebUI、mock/Ollama 联调） |
| Windows x64 | x86_64 | mingw-w64 交叉编译 / `scripts/build-windows.sh` | 5 exe 单 zip | **已实测**（v1.1.9 真机；v1.2.0-rc.1 待复测） |
| Linux arm64 | aarch64 | 未提供构建脚本 | 无 | 未支持、未实测 |
| Windows arm64 | aarch64 | 未提供构建脚本 | 无 | 未支持、未实测 |
| Windows x86 | i686 | 未提供构建脚本 | 无 | 未支持、未实测 |
| macOS x64 / arm64 | x86_64 / aarch64 | 未提供构建脚本 | 无 | 未支持、未实测 |

## 已实测 / 未实测

| 能力 | Linux x64 | Windows x64 | 证据与说明 |
|---|---|---|---|
| 内核启动、事件日志、插件 RPC | ✅ 自动化 | ✅ v1.1.9 真机 | Linux `cargo test`；Windows 真机 serve/chat |
| bench 6 项门禁 | ✅ 6/6 | ✅ 6/6（v1.1.9） | 平台阈值见下表 |
| 项目级工作区 / `.featheagent` | ✅ 自动化 | ✅ v1.1.9 真机 | 路径去重、S3 写盘 |
| TUI 交互 | ✅ PTY | ✅ v1.1.9 真机 | Markdown、`/stop`、`/settings` |
| WebUI | ✅ 浏览器 | ✅ v1.1.9 真机 | Markdown、图标、WS 空闲复验（Linux） |
| 零 Key 本地联网搜索 | ✅ 实测 | ✅ v1.1.9 真机 | 必应/百度/中新网回落、`web.search` 二次检索 |
| S3 工具三档 | ✅ 4/5 | ✅ 4/5（v1.1.9） | 真失败 0；脚本仍需真实 Ollama |
| 回合内 steer / `/compact` | ⚠️ 有单测，未两回合实测 | ⚠️ 未构造场景 | v1.1.9 报告明确未验证 |
| DeepSeek 计费搜索 | ❌ 未调用 | ❌ 未调用 | 需 API Key，按成功请求计费 |
| 高 DPI / 中文渲染 | ⚠️ 常规 DPI | ❌ 未实测 | Windows 高 DPI 待复测 |
| 网络盘 / 同步盘 / 24h 浸泡 | ❌ 未实测 | ❌ 未实测 | 不作为发布结论 |
| macOS / arm64 | ❌ 未支持 | ❌ 未支持 | 无构建产物 |

## 快速开始（Linux x64）

要求 Rust 1.80+，无其他运行时依赖。

```bash
scripts/build-linux.sh                                  # 版本门禁 + --locked 构建
./target/release/featheragent serve --root .          # 内核 + UDS
./target/release/featheragent-tui --root .            # TUI（另开终端）
./target/release/featheragent-webui --root . --app    # WebUI（Chromium --app 独立窗口）
```

出厂插件在 `plugins/`（provider-mock / provider-ollama / tool / storage / search）；`serve` 扫描 `<安装根>/plugins`，默认 `provider-mock`。接真实 Ollama：先 `ollama serve`，再 `serve --provider provider-ollama --model qwen2.5:7b`。WebUI 右上角“设置”或 TUI `/settings` 可在线切换 provider、模型、温度、上下文参数与工作区；WebUI 顶栏“插件”和 TUI `/plugins` 提供插件安装/启停，设置左侧栏会为每个已登记插件生成页面。模型下拉显示 `provider.models` 返回的全部模型，保存后经 `config.set` 即时应用。`serve` 启动会立即打印 banner 并写 `runtime/ipc/starting` / `ready`，首次启动较慢时 `chat` 会明确提示“内核未就绪（启动中）”。

三产物打包：`scripts/package.sh`，输出 `dist/*.tar.gz` + `sha256` + manifest；体积门禁（内核 ≤ 8 MiB）在脚本内断言。

运行时会在安装根下生成 `data/`、`workspace/`、`runtime/`；可用 `--data/--workspace/--runtime` 分别外置，或在设置页把 `workspace_root` 指向安装目录外（保存后重启 `serve` 生效）。`workspace_root` 是**项目容器**：容器下每个项目目录才是工作区，各自拥有 `.featheagent/`（`worklog.md` / `memory.md` / `README.md`）；worklog 由内核在回合结束时自动追加真实 UTC 时间、目标与工具，模型不手写时间。运行中可 `/stop` 紧急停止并保留上下文，直接输入文本会作为 `chat.inject` 引导当前回合，`/compact` 手动压缩旧上下文。TUI 进入会话后 ↑↓ 翻阅对话、Esc 返回会话列表；←/→/Home/End 可编辑输入，CJK 光标按显示宽度对齐。联网搜索默认走内置 `search` 插件（`search_provider=local`），无需 API Key；设置页可切到 DeepSeek 或关闭。

## 快速开始（Windows x64 测试包）

`FeatherAgent-1.2.0-rc.1-windows-x64.zip` 解压到有写权限的目录（不要放 `Program Files`），然后：

```powershell
.\featheragent.exe help                       # 先看命令表
.\featheragent.exe serve --root .
.\featheragent-tui.exe --root .              # 另开终端
.\featheragent-webui.exe --root . --app        # 自动找 Chrome/Edge
.\featheragent.exe chat --root . --text "hello"   # 无 WebUI/TUI 的脚本通道
```

5 个 exe 只依赖 Windows 系统 DLL（无 VC++ 运行库 / 无 MinGW 运行时）。`serve` 启动会立即打印 banner；首次启动可能 1–2 分钟（杀毒软件扫描插件二进制），期间会打印 `[init]` 进度；`runtime\ipc\ready` 未出现时 `chat` 会提示“内核未就绪（启动中）”，不是通道故障。构建：`scripts/build-windows.sh`（mingw-w64 + `x86_64-pc-windows-gnu` + 静态 CRT）；打包：`scripts/package-windows.sh`。详见 [Windows 测试说明](docs/Windows测试说明.md)。

## 平台门禁差异（v1.2.0-rc.1）

| 门禁 | Linux | Windows | 说明 |
|---|---|---|---|
| 进程内启动 P50 | < 20 ms | **< 100 ms** | Windows 冷启动含更多系统调用开销 |
| 事件写入 P95 | < 100 µs | **< 10 ms** | NTFS `FlushFileBuffers` + 实时防护导致一个数量级差异，属环境特性 |
| 内核体积 | ≤ 8 MB | ≤ 8 MB | 共用 |
| 空闲 RSS | ≤ 15 MB | ≤ 15 MB | 共用 |
| 上下文组装 P95 | < 30 ms | < 30 ms | 共用 |
| RPC P95 | < 1 ms | < 1 ms | 共用 |

`bench --json` 会返回 `platform`、`boot_limit_ms`、`write_limit_us`，不会把 Windows 固定标红。

## 参与与许可

- 贡献指南：[CONTRIBUTING.md](CONTRIBUTING.md)
- 安全问题：[SECURITY.md](SECURITY.md)（不要公开贴攻击细节/密钥）
- 行为准则：[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)
- 分支保护：[main 规则说明](.github/BRANCH_PROTECTION.md)
- 变更记录：[CHANGELOG.md](CHANGELOG.md)
- 本地 CI：[scripts/ci.sh](scripts/ci.sh)（`--with-windows` 追加交叉编译）
- 许可证：[MIT](LICENSE)

## 文档

- **[总体设计 v1.1](docs/FeatherAgent架构设计文档-v1.1.md)** —— 全文（含 v1.0 → v1.1 修订说明、勘误 R1、技术清单、路线图与风险对策）
- **[验证报告](docs/验证报告.md)** —— 测试矩阵、实测门禁数字、已修缺陷、未测项与偏差
- **[前端契约 · Wire Format](docs/前端契约-WireFormat.md)** —— 4 字节小端长度 + MsgPack/JSON、端点发现、握手顺序、错误码
- **[v1.2.0-rc.1 变更说明](docs/v1.2.0-rc.1-变更说明.md)** —— Q1/Q2、双平台版本/构建对齐、MIT 与公开仓库材料、平台矩阵
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

`v1.2.0-rc.1` 是公开仓库候选版：修复 v1.1.9 真机复测的 Q1/Q2 小项，统一 Linux/Windows 的版本源、`--locked` 构建与 PE 版本资源，补齐 MIT LICENSE、SECURITY、CONTRIBUTING、CODE_OF_CONDUCT、CHANGELOG、本地 CI 脚本与 CODEOWNERS。核心能力沿用 v1.1.9：项目级工作区路径去重、`/stop` 状态、TUI/WebUI Markdown、羽毛图标、零 Key 本地联网搜索（默认 `search_provider=local`，DeepSeek 保留为可选计费后端）。Linux 自动化测试 **86/86** 通过（新增 `json_u64` 字符串计数容错用例；另有 1 个需公网的集成用例单独跑通）。

Windows x64 已完成 v1.1.3～v1.1.9 九轮真机演进。v1.2.0-rc.1 的 Windows 包来自交叉编译，自动化测试只在 Linux 跑；Q1/Q2、Markdown、图标、本地搜索、项目路径与高 DPI/中文渲染仍需真机复测。Wine 只作启动冒烟，boot/RSS 受 Wine 开销影响不作 Windows 结论。本轮未实测项见上表。

