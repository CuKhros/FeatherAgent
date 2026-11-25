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
| Windows x64 | x86_64 | mingw-w64 交叉编译 / `scripts/build-windows.sh` | 5 exe 单 zip | **已实测**（v1.1.9 真机；v1.2.1-rc.3.5 待复测） |
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

TUI 与 WebUI 只提供中文界面，不再提供 `--lang` 切换，也不在二进制内附带英文界面资源。内核本身不依赖语言；文档仍保留中英双版。

三产物打包：`scripts/package.sh`，输出 `dist/*.tar.gz` + `sha256` + manifest；体积门禁（内核 ≤ 8 MiB）在脚本内断言。

运行时会在安装根下生成 `data/`、`workspace/`、`runtime/`；可用 `--data/--workspace/--runtime` 分别外置，或在设置页把 `workspace_root` 指向安装目录外（保存后重启 `serve` 生效）。`workspace_root` 是**项目容器**：容器下每个项目目录才是工作区，各自拥有 `.featheagent/`（`worklog.md` / `memory.md` / `README.md`）；worklog 由内核在回合结束时自动追加真实 UTC 时间、目标与工具，模型不手写时间。运行中可 `/stop` 紧急停止并保留上下文，直接输入文本会作为 `chat.inject` 引导当前回合，`/compact` 手动压缩旧上下文。TUI 进入会话后 ↑↓ 翻阅对话、Esc 返回会话列表；←/→/Home/End 可编辑输入，CJK 光标按显示宽度对齐。联网搜索默认走内置 `search` 插件（`search_provider=local`），无需 API Key；设置页可切到 DeepSeek 或关闭。

## 快速开始（Windows x64 测试包）

`FeatherAgent-1.2.1-rc.3.5-windows-x64.zip` 解压到有写权限的目录（不要放 `Program Files`），然后：

```powershell
.\featheragent.exe help                       # 先看命令表
.\featheragent.exe serve --root .
.\featheragent-tui.exe --root .              # 另开终端
.\featheragent-webui.exe --root . --app        # 自动找 Chrome/Edge
.\featheragent.exe chat --root . --text "hello"   # 无 WebUI/TUI 的脚本通道
```

5 个 exe 只依赖 Windows 系统 DLL（无 VC++ 运行库 / 无 MinGW 运行时）。`serve` 启动会立即打印 banner；首次启动可能 1–2 分钟（杀毒软件扫描插件二进制），期间会打印 `[init]` 进度；`runtime\ipc\ready` 未出现时 `chat` 会提示“内核未就绪（启动中）”，不是通道故障。构建：`scripts/build-windows.sh`（mingw-w64 + `x86_64-pc-windows-gnu` + 静态 CRT）；打包：`scripts/package-windows.sh`。详见 [Windows 测试说明](docs/Windows测试说明.zh-CN.md)。

## 平台门禁差异（v1.2.1-rc.3.5）

| 门禁 | Linux | Windows | 说明 |
|---|---|---|---|
| 热启动（进程内）P50 | < 20 ms | **< 100 ms** | 同一安装根，预热 1 次丢弃 + 5 次样本 |
| 冷启动 spawn→socket P50 | < 100 ms | **< 250 ms** | 真实 `serve` 子进程；Windows 含 DLL 加载与实时防护 |
| 事件写入 P95 | < 100 µs | **< 10 ms** | NTFS `FlushFileBuffers` + 实时防护导致一个数量级差异，属环境特性 |
| 内核体积 | ≤ 8 MB | ≤ 8 MB | 共用 |
| 空闲 RSS | ≤ 15 MB | ≤ 15 MB | 共用 |
| 上下文组装 P95 | < 30 ms | < 30 ms | 共用 |
| RPC P95 | < 1 ms | < 1 ms | 共用 |

`bench --json` 会返回 `platform`、`boot_mode`、`boot_samples_ms`、`boot_sample_count`、`boot_ms_p50`/`boot_ms_p90`、`cold_spawn_ms_p50`、`boot_limit_ms`、`cold_spawn_limit_ms` 与 `write_limit_us`；`metrics.env` 带 `mem_load_pct`、`reliability`，整机内存超过 85% 时附 `note`。不会把 Windows 固定标红。

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

`v1.2.1-rc.3.5` 修复 rc.3.4 真机复测的 M1/M3。guide 不再把占位符示例当成任务内容：`GUIDE_SYSTEM` 改用 `__FILL_*__` 形态，只讨论占位符/示例的条目会被丢弃并打 `review.warn`，真实条目保留；`must_include` 接受 `["SYN","ACK"]` 这类数组，并要求每个短语都出现（M1）。「简洁明了」这类主观判据标记为 advisory，不进入 hardcheck 与 judge；整份清单完全没有可执行判据时，重试用尽后直接失败，不再因为条目非空而放行（M3）。S3 t6 提示词明确要求先调用一次 `tool.fs.read` 再回答（M4）。rc.3.4 已完成判据真实化、feedback 不回显字面短语，并把每次 guide/judge 输出落到 `data/review-artifacts/<session>/`。

`v1.2.1-rc.3.2` 修掉 rc.3.1 真机复测的三条问题，不动架构：`min_points` 不再硬拒绝，而是作为软判据交 judge 复核（同时放宽中文要点计数并打 `review.warn`）；review feedback 用 `〔1〕` 序号，替代会被 exec 抄进答案的裸 `c1:`；`bench --json` 增加 `metrics.env` 环境指纹，便于跨时段对比数字。

`v1.2.1-rc.3.1` 保留 rc.3 的架构，只修真机复测暴露的问题：guide 现在拿到明确的 JSON schema 与 few-shot，校验错误会回注进重试而不是原样重发；只有描述项的判据先重试一次，仍不合格就交给 judge 逐条评估并打 `review.warn`，不再整轮 degraded。插件设置新增 `options_source`，模型字段显示由本地模型服务填充的真下拉，同时仍可手动键入；TUI 插件页任意字段 Enter 即保存；两端权限申请都显示完整命令/参数；产品显示名改为 **FeatherAgent**（二进制与路径不变）。

`v1.2.1-rc.3` 把插件设置改成声明式：插件在 `plugins/<name>/plugin.json` 的 `manifest.settings` 里写清 root、标题、说明和字段（`bool|int|number|string|enum`，带区间/选项），内核通过 `plugin.settings` RPC 原样转述，并在 `config.set` 里按声明校验，TUI 与 WebUI 都能通用渲染任意插件的表单。同时修掉 rc.2.1 复测的 H1–H4：短语判据四类硬校验、judge evidence 必须是答案中连续 ≥10 字原文、畸形工具名返回“未知工具 + 可用清单”的纠错，S3 t5/t6 明确只调用一次工具。

Linux 自动化共 **125 个测试：124 通过、1 个默认 ignored**（62 lib、22 plugin、2 kernel、3 TUI、3 WebUI、2 golden、31 integration）；ignored 的需公网用例单独运行通过。`fmt` 与 `clippy -D warnings` 通过。设置页两端都实测：TUI 用 PTY（review-gate 页出现“本地模型 1 个；←/→ 选择”，值字段 Enter 保存后 `config.json` 的 `max_rounds` 由 2 变 3）；WebUI 走 WebSocket 复测桥层（`plugin.settings` 返回三个 `options_source:"models"` 的模型字段，`tool.shell` 确认请求带 `arguments` 与 `$ echo hi` 的 preview，拒绝成功）。

Windows x64 已完成 v1.1.3～v1.1.9 九轮真机演进，并经过 rc.3.4 复测确认 provider body、温度拒绝、banner、R1 门禁、review-gate 分流、判据真实化、生成物与无泄漏 feedback。rc.3.4 复测通过 2/5、degraded 1/5：a2 的 guide 持续写关于 `<...>` 占位符的判据，三次原样重试后降级（M1）；a1 的「简洁明了」让 judge 三轮摇摆成 unapproved（M3）。本候选版硬化 guide 示例、丢弃元判据、把主观项降为 advisory，并支持数组 `must_include`；S3 t6 也加了硬性工具调用提示。v1.2.1-rc.3.5 的 Windows 包来自交叉编译，自动化测试只在 Linux 跑；M1/M3/M4 与此前遗留仍需真机复测。Wine 只作启动冒烟，boot/RSS 受 Wine 开销影响不作 Windows 结论。
