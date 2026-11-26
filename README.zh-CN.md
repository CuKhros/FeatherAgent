# FeatherAgent（轻羽）

> 中文版。英文主文档：[README.md](README.md)。

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

面向 **1B~14B 本地小模型**的「上下文引擎」+ 一个**体积极小、零运行时依赖**的可移植 Agent 内核。

> **主力实现 by Agent。** 本仓库的代码、测试、文档与发布流程主要由一个 AI Agent 完成并维护；人工主要负责方向、验收与真机验证。

**存在意义：把机械重复的步骤分给本地小模型去做。** 上层编排方留下需要判断的环节，把量大、枯燥、低风险的活儿交给同机运行的 1B~14B 模型——不绕云端、不按 token 计费，也不用让大模型背着这些上下文。

## 三个卖点

1. **小模型上下文工程** —— 技术差异所在，研发重心（四层压缩、预算器、容错解析、KV Cache 友好前缀）
2. **极致轻的内核** —— Rust 单二进制，目标 **≤ 8 MB**，空闲 RSS **≤ 15 MB**，热启动 **< 20 ms**
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
本地推理运行时（外部）：Ollama / llama.cpp / NInfer / LM Studio / vLLM / OpenAI 兼容服务
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
| Windows x64 | x86_64 | mingw-w64 交叉编译 / `scripts/build-windows.sh` | 10 exe 单 zip（5 中文 + 5 英文 `-en`） | **已实测**（v1.1.9 真机；rc.4.2–rc.4.7 已复测；v1.2.1-rc.4.8 包待复测） |
| Linux arm64 | aarch64 | 未提供构建脚本 | 无 | 未支持、未实测 |
| Windows arm64 | aarch64 | 未提供构建脚本 | 无 | 未支持、未实测 |
| Windows x86 | i686 | 未提供构建脚本 | 无 | 未支持、未实测 |
| macOS x64 / arm64 | x86_64 / aarch64 | 未提供构建脚本 | 无 | 未支持、未实测 |

## 已实测 / 未实测

| 能力 | Linux x64 | Windows x64 | 证据与说明 |
|---|---|---|---|
| 内核启动、事件日志、插件 RPC | ✅ 自动化 | ✅ v1.1.9 真机 | Linux `cargo test`；Windows 真机 serve/chat |
| bench 7 项门禁 | ✅ 7/7（v1.2.1） | ✅ 6/6（v1.1.9） | v1.2.1 改为热/冷启动分开计量；Windows 7/7 待复测 |
| `review-gate` 循环插件 | ✅ 集成测试 | ⚠️ 未实测 | guide/exec/judge 重答、事件、WebUI 设置 |
| 本地 provider + Bearer Key | ✅ mock HTTP 集成 | ⚠️ 未实测 | llama.cpp/NInfer/LM Studio/vLLM/OpenAI 兼容；未接真实服务 |
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

出厂插件在 `plugins/`：`provider-mock`、`provider-ollama`、`provider-llamacpp`、`provider-ninfer`、`provider-lmstudio`、`provider-vllm`、通用 `provider-openai`、`review-gate`、`tool`、`storage` 与 `search`。`serve` 扫描 `<安装根>/plugins`，默认 `provider-mock`。接真实 Ollama：先 `ollama serve`，再 `serve --provider provider-ollama --model qwen2.5:7b`。Ollama 原生 `/api/*` 与 `/v1` Base 均可；llama.cpp/NInfer/LM Studio/vLLM 走各自 OpenAI 兼容 `/v1` 端点，Base URL 会自动归一化。WebUI 右上角“设置”或 TUI `/settings` 可在线切换 provider、模型、温度、上下文参数、工作区与可选 Bearer API Key；Key 只发给当前 provider，仅存本机 `config.json`；WebUI 顶栏“插件”和 TUI `/plugins` 提供插件安装/启停，设置左侧栏会为每个已登记插件生成页面。模型下拉显示 `provider.models` 返回的全部模型，保存后经 `config.set` 即时应用。`serve` 启动会立即打印 banner 并写 `runtime/ipc/starting` / `ready`，首次启动较慢时 `chat` 会明确提示“内核未就绪（启动中）”。

中文可执行文件仍是默认版本。每个顶层二进制都另有英文版，由文件名后缀 `-en` 选择（`featheragent-en`、`featheragent-tui-en`、`featheragent-webui-en`、`feather-plugin-en`、`feather-storage-en`）；不提供 `--lang` 切换。英文 TUI/WebUI 内含独立的运行时文案与资源，plugin/storage 的 `-en` 是纯协议别名；中文版行为不变。内核本身不依赖语言；文档仍保留中英双版。

**文件附件。** WebUI 输入区的 `+` 会打开系统文件选择框；TUI 用 `@<路径>` 传入文件（路径含空格时写 `@"..."`）。前端先核实文件存在，再调用 `file.prepare` / `file.attach`；内核复核存在性，若文件在安装目录之外则复制到 `workspace/default/uploads/`，给模型的路径是内核返回的路径，而不是浏览器临时路径或安装目录外的原路径。上传字节上限 8 MiB，走 HTTP RPC（base64），请求体上限 12 MiB。搜索/新闻/抓取结果同时写入 `workspace/<项目>/.featheagent/search/<时间戳>-<工具>.json`，工具结果里带 `artifact` 路径。

三产物打包：`scripts/package.sh`，输出 `dist/*.tar.gz` + `sha256` + manifest；体积门禁（内核 ≤ 8 MiB）在脚本内断言。

运行时会在安装根下生成 `data/`、`workspace/`、`runtime/`；可用 `--data/--workspace/--runtime` 分别外置，或在设置页把 `workspace_root` 指向安装目录外（保存后重启 `serve` 生效）。`workspace_root` 是**项目容器**：容器下每个项目目录才是工作区，各自拥有 `.featheagent/`（`worklog.md` / `memory.md` / `README.md`）；worklog 由内核在回合结束时自动追加真实 UTC 时间、目标与工具，模型不手写时间。运行中可 `/stop` 紧急停止并保留上下文，直接输入文本会作为 `chat.inject` 引导当前回合，`/compact` 手动压缩旧上下文。TUI 进入会话后 ↑↓ 翻阅对话、Esc 返回会话列表；←/→/Home/End 可编辑输入，CJK 光标按显示宽度对齐。联网搜索默认走内置 `search` 插件（`search_provider=local`），无需 API Key；设置页可切到 DeepSeek 或关闭。

## 快速开始（Windows x64 测试包）

`FeatherAgent-1.2.1-rc.4.8-windows-x64.zip` 解压到有写权限的目录（不要放 `Program Files`），然后：

```powershell
.\featheragent.exe help                       # 中文命令表
.\featheragent-en.exe help                    # 英文命令表
.\featheragent.exe serve --root .
.\featheragent-tui.exe --root .              # 中文 TUI（另开终端）
.\featheragent-tui-en.exe --root .           # 英文 TUI
.\featheragent-webui.exe --root . --app        # 中文 WebUI（自动找 Chrome/Edge）
.\featheragent-webui-en.exe --root . --app     # 英文 WebUI
.\featheragent.exe chat --root . --text "hello"   # 无 WebUI/TUI 的脚本通道
```

10 个 exe 只依赖 Windows 系统 DLL（无 VC++ 运行库 / 无 MinGW 运行时）。`serve` 启动会立即打印 banner；首次启动可能 1–2 分钟（杀毒软件扫描插件二进制），期间会打印 `[init]` 进度；`runtime\ipc\ready` 未出现时 `chat` 会提示“内核未就绪（启动中）”，不是通道故障。构建：`scripts/build-windows.sh`（mingw-w64 + `x86_64-pc-windows-gnu` + 静态 CRT）；打包：`scripts/package-windows.sh`。详见 [Windows 测试说明](docs/Windows测试说明.zh-CN.md)。

## 平台门禁差异（v1.2.1-rc.4.8）

| 门禁 | Linux | Windows | 说明 |
|---|---|---|---|
| 热启动（进程内）P50 | < 20 ms | **< 100 ms** | 同一安装根，预热 1 次丢弃 + 5 次样本 |
| 冷启动 spawn→socket P50 | < 100 ms | **< 250 ms** | 真实 `serve` 子进程；Windows 含 DLL 加载与实时防护 |
| 事件写入 P95 | < 100 µs | **< 10 ms 严格；10–20 ms advisory** | NTFS `FlushFileBuffers` + 实时防护导致一个数量级差异；边缘抖动标 advisory，超过 20 ms 仍 fail |
| 内核体积 | ≤ 8 MB | ≤ 8 MB | 共用 |
| 空闲 RSS | ≤ 15 MB | ≤ 15 MB | 共用 |
| 上下文组装 P95 | < 30 ms | < 30 ms | 共用 |
| RPC P95 | < 1 ms | < 1 ms | 共用 |

`bench --json` 返回四个顶层对象：`metrics`、`gates`、`diagnostics`、`advisory_gates`。**`gates` 只放布尔判定项**（`binary_le_8mb`、`rss_le_15mb`、`assemble_p95_lt_30ms`、`rpc_p95_lt_1ms`、`metrics_consistent`、`boot_lt_*`、`cold_spawn_lt_*`、`write_p95_lt_*`），按“数 False”统计是安全的。`diagnostics` 放 `platform`、`boot_gate_raw/status/trusted`、`write_gate_raw/status`、`reliability`、`boot_mode`、`boot_limit_ms`、`cold_spawn_mode`、`cold_spawn_limit_ms`、`write_limit_us`；读写入状态必须从 `diagnostics` 读，不能从 `gates` 读。`metrics.env` 带 `mem_load_pct`、`cpu_load_pct`、`cpu_load_source`，内存或 CPU 超过 85% 时附 `note`；`reliability` 取内存/CPU 更差者（≥70% medium、≥85% low）。不会把 Windows 固定标红。

## 参与与许可

- 贡献指南：[CONTRIBUTING.zh-CN.md](CONTRIBUTING.zh-CN.md)
- 安全问题：[SECURITY.zh-CN.md](SECURITY.zh-CN.md)（不要公开贴攻击细节/密钥）
- 行为准则：[CODE_OF_CONDUCT.zh-CN.md](CODE_OF_CONDUCT.zh-CN.md)
- 分支保护：[main 规则说明](.github/BRANCH_PROTECTION.md)
- 变更记录：[CHANGELOG.zh-CN.md](CHANGELOG.zh-CN.md)
- 本地 CI：[scripts/ci.sh](scripts/ci.sh)（`--with-windows` 追加交叉编译）
- 许可证：[MIT](LICENSE)

## 文档

- **[总体设计 v1.1](docs/FeatherAgent架构设计文档-v1.1.zh-CN.md)** —— 全文（含 v1.0 → v1.1 修订说明、勘误 R1、技术清单、路线图与风险对策）
- **[验证报告](docs/验证报告.zh-CN.md)** —— 测试矩阵、实测门禁数字、已修缺陷、未测项与偏差
- **[前端契约 · Wire Format](docs/前端契约-WireFormat.zh-CN.md)** —— 4 字节小端长度 + MsgPack/JSON、端点发现、握手顺序、错误码
- **[v1.2.1-rc.4.8 变更说明](docs/v1.2.1-rc.4.8-变更说明.zh-CN.md)** —— 扩展公式变体与 ML10 分步判据、强杀残留 turn 的 run id 隔离，以及 rc.4.7 报告/CSV 归档（[English](docs/v1.2.1-rc.4.8-Changes.md)）
- **[v1.2.1-rc.4.7 变更说明](docs/v1.2.1-rc.4.7-变更说明.zh-CN.md)** —— judge evidence 中英双门限、工具拒绝提示、数学推导完整性提示，以及两张数学卷归档（[English](docs/v1.2.1-rc.4.7-Changes.md)）
- **[v1.2.1-rc.4.6 变更说明](docs/v1.2.1-rc.4.6-变更说明.zh-CN.md)** —— Windows 路径的 review JSON 容错、可配置输出上限与 `finish=length` 检测、只含布尔判定项的 `bench` gates 与 `diagnostics`、纳入 CPU 的 reliability，以及生成器/解析层/Web bundle 重构（[English](docs/v1.2.1-rc.4.6-Changes.md)）
- **[代码堆积审计 v1.2.1-rc.4.6](docs/代码堆积审计-v1.2.1-rc.4.6.zh-CN.md)** —— 五处有证据的技术债、作为基线的过去形态、本轮重构范围与刻意未动的部分（[English](docs/Code-Audit-v1.2.1-rc.4.6.md)）
- **[v1.2.1-rc.4.5 变更说明](docs/v1.2.1-rc.4.5-变更说明.zh-CN.md)** —— 写文件任务跳过硬判据并全交 judge、Windows 写入门禁 advisory、L17 粗体编号识别，以及 rc.4.4 报告与 5 份试卷归档（[English](docs/v1.2.1-rc.4.5-Changes.md)）
- **[v1.2.1-rc.4.4 变更说明](docs/v1.2.1-rc.4.4-变更说明.zh-CN.md)** —— bench 停止/join 有界、临时目录清理、写文件判据交 judge、XML 开标签恢复、对比轴覆盖与 TUI 到顶滚动夹紧（[English](docs/v1.2.1-rc.4.4-Changes.md)）
- **[v1.2.1-rc.4.3 变更说明](docs/v1.2.1-rc.4.3-变更说明.zh-CN.md)** —— 冷启动单次 IPC 探测、代码与文件输出区分、对比轴覆盖、写文件 review 证据，以及精选模型报告归档（[English](docs/v1.2.1-rc.4.3-Changes.md)）
- **[模型测试报告归档](docs/model-test-reports/README.zh-CN.md)** —— 13 份含可复核本地模型数据的 Windows 真机报告，附结构化 CSV 与 7 份可复用试卷（[English](docs/model-test-reports/README.md)）
- **[v1.2.1-rc.4.2 变更说明](docs/v1.2.1-rc.4.2-变更说明.zh-CN.md)** —— XML 风格工具调用恢复、Windows bench RPC 连接重试、`advisory_gates` 拆分、简洁与覆盖的提示词区分（[English](docs/v1.2.1-rc.4.2-Changes.md)）
- **[v1.2.1-rc.4.1 变更说明](docs/v1.2.1-rc.4.1-变更说明.zh-CN.md)** —— 路径/文件型 `must_include` 豁免、纯文本工具调用兜底与别名归一化、`data/web-artifacts` 可审计副本、`rubric-final` 留档；并注明 4B 长流程边界（[English](docs/v1.2.1-rc.4.1-Changes.md)）
- **[v1.2.1-rc.4 变更说明](docs/v1.2.1-rc.4-变更说明.zh-CN.md)** —— `must_include` 匹配模式、主观判据/禁词校验、bench 唯一临时目录与结构化错误、reliability 感知启动门禁、重复工具调用抑制、搜索落盘、WebUI/TUI 文件附件（[English](docs/v1.2.1-rc.4-Changes.md)）
- **[v1.2.1-rc.3.6 变更说明](docs/v1.2.1-rc.3.6-变更说明.zh-CN.md)** —— `must_include` 只收术语并按词项匹配、percentile 内部排序且 bench P50/P90 同源、空响应/工具格式重试与纠错、fixture 禁词整条丢弃、全部顶层二进制英文版（[English](docs/v1.2.1-rc.3.6-Changes.md)）
- **[v1.2.1-rc.3.5 变更说明](docs/v1.2.1-rc.3.5-变更说明.zh-CN.md)** —— 占位符元判据自动丢弃、主观判据降级为 advisory 且不进 judge、数组 `must_include`、S3 t6 工具调用提示强化（[English](docs/v1.2.1-rc.3.5-Changes.md)）
- **[v1.2.1-rc.3.4 变更说明](docs/v1.2.1-rc.3.4-变更说明.zh-CN.md)** —— guide schema 占位符与校验（不再抄 `REVIEW-FINAL`）、feedback 不回显字面短语、Guide/Judge 生成物落盘到 `data/review-artifacts`（[English](docs/v1.2.1-rc.3.4-Changes.md)）
- **[v1.2.1-rc.3.3 变更说明](docs/v1.2.1-rc.3.3-变更说明.zh-CN.md)** —— 判据分流（字面/禁词/计数/正则归 hardcheck，只有描述型进 judge）、正则判据、缺项回注、串行 majority 投票、热启动 P90 与样本数、内存压力可信度注记（[English](docs/v1.2.1-rc.3.3-Changes.md)）
- **[v1.2.1-rc.3.2 变更说明](docs/v1.2.1-rc.3.2-变更说明.zh-CN.md)** —— `min_points` 降级为软判据（`review.warn` + judge 复核）、中文分号/加粗小标题要点计数、feedback 改 `〔1〕` 序号不再带裸 `c1:`、bench `metrics.env` 环境指纹
- **[v1.2.1-rc.3.1 变更说明](docs/v1.2.1-rc.3.1-变更说明.zh-CN.md)** —— guide 给 schema + 错误回注重试 + 软判据 judge-only 与 `review.warn`、模型字段从 `provider.models` 下拉、TUI 插件页任意字段 Enter 保存、权限申请显示完整命令、显示名改为 **FeatherAgent**
- **[v1.2.1-rc.3 变更说明](docs/v1.2.1-rc.3-变更说明.zh-CN.md)** —— 插件自述设置（`manifest.settings` + `plugin.settings`）、TUI/WebUI 通用表单、短语判据四类硬校验、judge evidence 必须是答案原文、未知工具纠错
- **[v1.2.1-rc.2.1 变更说明](docs/v1.2.1-rc.2.1-变更说明.zh-CN.md)** —— Ollama `/api/tags` 模型列表、TUI review-gate 设置、字面 rubric/硬校验、judge 重试、`chat.finished` status
- **[v1.2.1-rc.2 变更说明](docs/v1.2.1-rc.2-变更说明.zh-CN.md)** —— provider 空 body 修复、review 温度校验、生效配置 banner
- **[v1.2.1-rc.1 变更说明](docs/v1.2.1-rc.1-变更说明.zh-CN.md)** —— R1 热/冷启动拆分、review-gate、本地 provider、API Key 与测试
- **[v1.2.0-rc.1 变更说明](docs/v1.2.0-rc.1-变更说明.zh-CN.md)** —— Q1/Q2、双平台版本/构建对齐、MIT 与公开仓库材料、平台矩阵
- **[v1.1.9 Windows 修复对照](docs/v1.1.9-Windows修复说明.zh-CN.md)** —— P1 路径双重嵌套、`/stop` 状态、TUI/WebUI Markdown、羽毛图标、零 Key 本地联网搜索
- **[v1.1.8 Windows 修复对照](docs/v1.1.8-Windows修复说明.zh-CN.md)** —— 项目级工作区与每项目 `.featheagent`、TUI 设置侧栏、紧急停止/引导/压缩、联网搜索，以及 v1.1.7 复测 N1/N2/N3/N4/N6/N7
- **[v1.1.7 Windows 修复对照](docs/v1.1.7-Windows修复说明.zh-CN.md)** —— v1.1.6 复测 NEW-K、TUI 双模式/光标、外置工作区与 `.featheagent`、插件管理与设置侧栏
- **[v1.1.6 Windows 修复对照](docs/v1.1.6-Windows修复说明.zh-CN.md)** —— v1.1.5 复测 NEW-H～NEW-J、启动就绪标记、配置单字段原子更新、模型下拉全量展示
- **[v1.1.5 Windows 修复对照](docs/v1.1.5-Windows修复说明.zh-CN.md)** —— v1.1.4 复测 NEW-E～NEW-G、工具熔断粒度与 WebUI/TUI 设置页
- **[v1.1.4 Windows 修复对照](docs/v1.1.4-Windows修复说明.zh-CN.md)** —— v1.1.3 复测 NEW-A～NEW-D 与 WebUI 更名
- **[v1.1.3 Windows 修复对照](docs/v1.1.3-Windows修复说明.zh-CN.md)** —— 上一轮 NEW-1～NEW-4 的修复记录
- **[v1.1.2 Windows 修复对照](docs/v1.1.2-Windows修复说明.zh-CN.md)** —— 上一轮 BUG-1～BUG-9 的修复记录

> 文档中所有「目标值」必须经 Spike / 阶段门禁实测后回填，未测不写数字。

## 状态

`v1.2.1-rc.4.8` 处理 rc.4.7 复测的 N40–N42。ML02 现在接受 LaTeX/Unicode/ζ 变体（`\dfrac`、`\zeta(2)`、`π²/6`、`(π^2)/6`、反写乘积），ML10 拆为换元、展开、降幂/倍角、`π/16` 四步计分。N42 已在 rc.4.7 复现（5 次里 1 次把旧回合的 `stopped` 交给下一个客户端），用每次 `chat.send` 的 run id 修复：旧回合静默退出，断连使旧 run 失效，`session.create` 清理旧 holder，CLI 按 `run` 过滤 `chat.finished`；rc.4.8 连续 5/5 强杀/重连干净。rc.4.7 复测同时确认 N37 的实际收益（放行 `tool.shell` 后数学小卷 19.5→21.5、大卷 40.7→41.6，FA 126s/446s）与 N38 的恢复（MS2 37→729、ML02 109→1568 字）；N36 仍需干净环境复测。评分仍是正则覆盖面，不是数值等价。

`v1.2.1-rc.4.7` 修复 rc.4.6 复测的 N36–N39。judge evidence 现在接受 6 个汉字、3 个英文词或 10 个字符的标识符，9 个字的 `三条要点已写入文件` 不再让写文件任务降级。工具被拒的 `tool.result` 增加 `hint`，明确要求模型不要重复调用、改用文字完成；系统提示要求数学/证明题写出完整推导。公开试卷库新增 `math-small`（8 题/25 分）与 `math-large`（20 题/56 分）；rc.4.6 的 FA 得分为 19.5/25、40.7/56，裸跑为 16.5/25、31.7/56。N37 仍是跑卷前提（希望 sympy 辅助时需放行 `tool.shell`）；N38 由提示词缓解，仍需真实模型复测；N39 是评分覆盖面限制。

`v1.2.1-rc.4.6` 修复 rc.4.5 复测的 N32–N35，并清掉代码审计确认的五处结构性技术债。judge 输出现在容忍 JSON 字符串里的 Windows 路径反斜杠（先标准解析，再只修字符串内部），证据路径统一用 `/`；`Config.max_output`（默认 2048）替换两处硬编码 512，并按 provider 传 `num_predict` / `max_tokens`，`finish=length` 以 `truncated` 暴露且让 review 角色失败重试。`bench --json` 的 `gates` 只保留布尔判定项，状态/原始读数/阈值移到 `diagnostics`；`reliability` 取内存与 CPU 更差者。review 解析层抽到 `src/review.rs`，中英 Rust UI 生成器按 rc.3.6 基线重建，英文 WebUI 改为带结构校验的独立 bundle。N32/N33/N35 已在 rc.4.6 真机复测确认。

`v1.2.1-rc.4.5` 修复 rc.4.4 复测的 N29–N31。明确的写文件任务不再要求 guide 产出硬判据：判据全部转 judge，`parse_rubric` 在 `file_task` 时提前剥掉 `must_include`/`min_points`/`pattern` 等确定性键，`must_include:["-"]` 不再触发“术语至少 2 字”校验；`review_prepare` 不再对写文件任务做“零硬判据”重试。Windows 写入门禁把 10–20 ms 的边缘抖动标为 `advisory`，保留 `write_gate_raw`，超过 20 ms 仍 fail；Linux 仍按 100 µs 严格判定。测试侧 L17 列表判据识别 `**1.**` 粗体编号。rc.4.5 新增 1 项单测，并把 `REVIEW_WRITE` 集成夹具改为复现 N29 的坏符号硬键。

`v1.2.1-rc.4.4` 修复 rc.4.3 复测的 N23–N27，并修掉 TUI 长对话到顶后的虚拟滚动延迟。`Server::stop` 改为 3 秒总预算重试命名管道唤醒，bench 的两处 join 改为 5 秒 deadline 并返回明确错误；bench 临时目录用 `Drop` 统一清理，启动时清掉超过 24 小时的旧 `fa-bench-*`。明确的写文件任务把 guide 判据全部转给 judge，证据带 `tool.fs.write path=... status=success` 与文件正文，不再因答案没有复述 guide 自造词而失败；文件已写入后模型返回畸形 XML 时用确定性收尾，不再整轮 failed。XML function 开标签漏 `>` 时按换行或参数前截断；对比题按机制/连接、可靠性/一致性、性能与开销、典型场景四轴逐维展开；中英 TUI 在 `draw` 里先把 `chat_scroll` 夹到 `max_scroll`，到顶后多按 Up 不再积累虚拟增量。N23–N27 仍需真机确认；N28 是测试侧粗体标题识别限制。

`v1.2.1-rc.4.3` 修复 rc.4.2 复测的四条问题。`serve` 的旧内核探测改用单次 `ipc::try_connect`，干净安装根不再先等满 3 秒管道重试；正常客户端仍保留有界重试。代码/文本/方案/译文默认留在答复里，只有明确要求保存或写入文件才调用文件工具。列举与比较的第一层条目对应用户要求的对比轴；用户未给轴时覆盖性能与开销。明确的写文件任务会把成功写入的路径与内容作为带标签证据交给 review-gate，检查文件而不是要求答复逐字复述；提示词也禁止写成功后反复读取。N15–N18 已由 rc.4.2 复测确认。

`v1.2.1-rc.4.2` 修复 rc.4.1 复测的四条问题。XML 风格的 function/parameter 工具调用改由上下文解析器恢复，Ollama provider 在流式报错时会先尝试从已累计文本抢救调用，而不是整轮 failed。bench 显式创建 RPC 目录，并跨过 Windows 命名管道创建窗口重试连接；`boot_gate_trusted` 移入 `advisory_gates`，低可信度主机不再被算成发布门禁失败。系统提示区分“默认简洁”和“列举/比较/穷举必须覆盖全部要点”。真机已确认 N10、N12、N13；N15–N18 仍需下一轮 Windows 复测。

`v1.2.1-rc.4` 修复 rc.3.6 复测的五条问题，并加入附件与搜索落盘能力。`must_include` 支持 `match: any|all|min`；多术语默认 `min`（至少 ceil(n/2) 项），hardcheck 报告覆盖了几项。主观判据禁止使用 `must_not_include`；禁词若出现在用户输入中，guide 会被判不合格。bench 每次使用唯一临时目录，失败返回 `artifact_missing` / `bench_failed` JSON；`reliability` 非 high 时启动门禁标为 `advisory`，同时保留 `boot_gate_raw`。同一模型回复里相同的工具调用只执行一次，每条 `tool.call` 带 `attempt` 与 `call_index`。搜索/新闻/抓取结果落盘为 JSON artifact；WebUI/TUI 附件在安装目录外时复制到默认工作区。

`v1.2.1-rc.3.6` 修复 rc.3.5 复测的四条问题，并为全部顶层二进制增加英文版。`must_include` 改为只收术语（2–12 字，禁止 `和/与/及/是/的` 等概念连接词或整句），hardcheck 不再要求整串连续子串，而是按空白/标点拆词项并要求每个词项出现；概念性写法在 guide 阶段被拒并重试。`percentile` 内部排序，bench 只算一次 P50/P90，JSON 与门禁新增 `boot_percentiles_consistent` / `metrics_consistent`。空响应最多两次升温重试，Ollama 畸形工具调用最多两次重试并回注 JSON 格式纠错，均发 `model.retry`；插件崩溃仍重启一次。`must_not_include` 命中 `REVIEW-FINAL`、`REVIEW-START`、`<...>`、`__FILL`、`{{`、`}}` 时整条丢弃并打 `review.warn`。五个中文可执行文件保持原名，`featheragent-en`、`featheragent-tui-en`、`featheragent-webui-en`、`feather-plugin-en`、`feather-storage-en` 与它们一起进入每个 Release 资产包。

`v1.2.1-rc.3.5` 修复 rc.3.4 真机复测的 M1/M3。guide 不再把占位符示例当成任务内容：`GUIDE_SYSTEM` 改用 `__FILL_*__` 形态，只讨论占位符/示例的条目会被丢弃并打 `review.warn`，真实条目保留；`must_include` 接受 `["SYN","ACK"]` 这类数组，并要求每个短语都出现（M1）。「简洁明了」这类主观判据标记为 advisory，不进入 hardcheck 与 judge；整份清单完全没有可执行判据时，重试用尽后直接失败，不再因为条目非空而放行（M3）。S3 t6 提示词明确要求先调用一次 `tool.fs.read` 再回答（M4）。rc.3.4 已完成判据真实化、feedback 不回显字面短语，并把每次 guide/judge 输出落到 `data/review-artifacts/<session>/`。

`v1.2.1-rc.3.2` 修掉 rc.3.1 真机复测的三条问题，不动架构：`min_points` 不再硬拒绝，而是作为软判据交 judge 复核（同时放宽中文要点计数并打 `review.warn`）；review feedback 用 `〔1〕` 序号，替代会被 exec 抄进答案的裸 `c1:`；`bench --json` 增加 `metrics.env` 环境指纹，便于跨时段对比数字。

`v1.2.1-rc.3.1` 保留 rc.3 的架构，只修真机复测暴露的问题：guide 现在拿到明确的 JSON schema 与 few-shot，校验错误会回注进重试而不是原样重发；只有描述项的判据先重试一次，仍不合格就交给 judge 逐条评估并打 `review.warn`，不再整轮 degraded。插件设置新增 `options_source`，模型字段显示由本地模型服务填充的真下拉，同时仍可手动键入；TUI 插件页任意字段 Enter 即保存；两端权限申请都显示完整命令/参数；产品显示名改为 **FeatherAgent**（二进制与路径不变）。

`v1.2.1-rc.3` 把插件设置改成声明式：插件在 `plugins/<name>/plugin.json` 的 `manifest.settings` 里写清 root、标题、说明和字段（`bool|int|number|string|enum`，带区间/选项），内核通过 `plugin.settings` RPC 原样转述，并在 `config.set` 里按声明校验，TUI 与 WebUI 都能通用渲染任意插件的表单。同时修掉 rc.2.1 复测的 H1–H4：短语判据四类硬校验、judge evidence 必须是答案中连续 ≥10 字原文、畸形工具名返回“未知工具 + 可用清单”的纠错，S3 t5/t6 明确只调用一次工具。

Linux 自动化共 **211 项测试：210 通过、1 个默认 ignored**（83 lib、34 plugin ×2、5 kernel、6 TUI、4 WebUI ×2、2 golden、38 integration）；英文 worker/WebUI 别名复用同一测试模块，因此 plugin 与 WebUI 用例各执行两次。ignored 的需公网用例需公网环境。`fmt` 与 `clippy -D warnings` 通过。Release bench：内核 **2,581,016 B（2.46 MiB）**；热启动 P50 **1.408 / 1.356 / 1.338 ms**；冷启动 P50 **5.786 / 5.432 / 5.320 ms**；RSS **3,712 / 3,584 / 3,456 KiB**；写入 P95 **2.844 / 2.824 / 2.794 µs**；8 项布尔门禁全绿，`diagnostics.platform:"linux"`、`write_gate_status:"pass"`、`write_gate_raw:true`；`reliability` 三次均为 high（`mem=17%,cpu=63%` / `mem=17%,cpu=62%` / `mem=17%,cpu=62%`），故 `boot_gate_trusted=true`、`boot_gate_status:"pass"`，热启动原始值远低于 20 ms。设置页已在 rc.3 两端实测：TUI 用 PTY，WebUI 走 WebSocket。

Windows x64 已完成 v1.1.3～v1.1.9 九轮真机演进，并经过 rc.4.1～rc.4.7 复测。rc.4.6 复测确认 N32–N35、暴露 N36–N39；rc.4.7 复测确认 N37 的实际收益（放行 `tool.shell` 后 FA 21.5/25、41.6/56，耗时 126s/446s）与 N38 的恢复，但因残留 turn 连续 `interrupted` 未能测到 N36，并暴露 N40（公式写法评分）、N41（部分分过粗）、N42（强杀后残留 turn）。rc.4.8 候选版扩展公式判据、把 ML10 拆成四步、用 run id 隔离旧回合。Windows 包含 10 个 exe（5 中文 + 5 英文 `-en`），来自交叉编译；自动化测试只在 Linux 跑。Wine 6.0.3 冒烟通过 `version`、`doctor`、三次 `bench --json`、`serve` + `chat`，以及一次 N42 强杀/重连；冷启动/RSS/写入门禁受 Wine 开销影响，不作 Windows 基线。真实的 Windows 命名管道强杀时序仍需用户真机复测。
