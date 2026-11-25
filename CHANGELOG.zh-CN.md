# Changelog

格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本遵循语义化版本。`v1.2.1-rc.3.6` 是当前公开仓库候选版，尚未替代稳定版。

## [1.2.1-rc.3.6] - 2026-11-25

### 修复
- N1：`must_include` 改为只收术语。`GUIDE_SYSTEM` 只允许专有名词、术语、文件名、命令或固定写法，2–12 字，禁止概念性描述、整句和连接词（`和/与/及/或/是/的/了`）。`MUST_INCLUDE_MAX_CHARS` 为 12；`must_not_include` 单独保留 20 字上限。`review.hardcheck` 按空白、`、`、`，`、`,`、`；`、`;`、`/` 拆词项，要求每个词项都出现；报告中的 `管理计算机硬件和资源` 会在 guide 阶段被拒，`管理计算机硬件 资源` 可在改写答案中通过。
- N2：`util::percentile` 内部排序。bench 只算一次 `boot_p50`/`boot_p90` 并复用，新增 `metrics.boot_percentiles_consistent` 与 `gates.metrics_consistent`；P50 ≤ P90 成为结构保证。
- N3：`run_turn` 对空响应最多重试 2 次（温度 0.70/0.75），对 Ollama 畸形工具调用最多重试 2 次并回注 JSON 格式纠错，均发 `model.retry`（`reason:"empty"` / `"tool_format"`）。插件崩溃仍重启一次；预算耗尽才发 `failed`。
- N4：`must_not_include` 命中 `REVIEW-FINAL`、`REVIEW-START`、`<...>`、`__FILL`、`{{`、`}}` 时整条丢弃并打 `review.warn`。

### 新增
- 全部五个顶层二进制的英文版：`featheragent-en`、`featheragent-tui-en`、`featheragent-webui-en`、`feather-plugin-en`、`feather-storage-en`。WebUI 用 `-en` 后缀选择英文资源；kernel/TUI 使用独立英文源码 target；plugin/storage 是纯协议别名。
- Windows zip 含 10 个 exe（5 中文 + 5 `-en`）；Linux core 含 8 个二进制（4 中文 + 4 `-en`）；WebUI 包同时含中英两版。
- 测试：`percentile_works_on_unsorted_samples_and_p50_le_p90`、`tool_format_error_detector_covers_ollama_and_json_errors`、`review_rubric_rejects_conceptual_must_include_with_connective`、`review_rubric_rejects_one_char_must_include`、`review_hardcheck_matches_include_terms_across_wording`、`review_rubric_drops_fixture_must_not_include_item`、`english_suffix_selects_english_assets`。

### 变更
- Linux 自动化：**162 次测试执行，161 通过、1 ignored**；英文 worker/WebUI 别名 target 重复执行 26 个 plugin 与 4 个 WebUI 用例。`fmt` 与 `clippy -D warnings` 通过。
- bench：热启动 P50 1.142/1.167/1.163 ms，P90 1.761/1.190/1.170 ms；冷启动 3.722/2.324/2.262 ms；门禁全绿。
- 没有改动 wire 消息、契约、provider、storage、tool 或插件设置；bench 字段为增量。

## [1.2.1-rc.3.5] - 2026-11-25

### 修复
- M1：guide 不再把占位符示例写成判据。`GUIDE_SYSTEM` 改用 `__FILL_*__` 形态，禁止在判据里讨论占位符/模板；`review.parse_rubric` 丢弃自我指涉的占位条目、保留真实条目。a2 的 rubric 现在保留 c1/c3、丢弃 c2、打 `review.warn`，整轮不再 degraded。
- M1：`must_include` 接受字符串数组（`["SYN","ACK"]`），hardcheck 逐项要求全部出现，不再静默退化为描述型。
- M3：主观描述判据（简洁、清晰、排版好看等）标记为 `advisory`，不进入 hardcheck 与 judge，并打 `review.warn`。内核在重试用尽后仍没有可执行判据时直接失败，不再因为条目非空就放行。
- M4：S3 t6 提示词明确要求先调用一次 `tool.fs.read`，禁止不调工具凭猜测回答。

### 新增
- `review.parse_rubric` 输出 `advisory` id 列表；`review.warn` 报告被丢弃的占位条目与主观降级。
- 测试：占位元判据清洗、主观 advisory、数组 `must_include`，以及集成 `review_gate_drops_placeholder_meta_item_and_approves`、`review_gate_subjective_criterion_is_advisory`。

### 变更
- `GUIDE_SYSTEM` 规则 8/9：示例只示范字段名；禁止主观判据，改用可计数检查。
- 没有改动 wire 消息、契约、provider、storage、tool 或插件设置。

## [1.2.1-rc.3.4] - 2026-11-25

### 修复
- L1：guide 不再抄 JSON schema。`GUIDE_SYSTEM` 的示例值全部改成尖括号占位符（`<...>`），删掉 mock 专用 `REVIEW-FINAL`；`review.parse_rubric` 拒绝 `criterion` / `check` / `must_include` / `must_not_include` 里的占位文本。`must_include` 为 `REVIEW-FINAL` / `REVIEW-START` 且未出现在用户输入时直接拒绝。该检查只限这两个 mock 标记，避免误杀 `HTTP`、`JSON` 这类任务术语。
- L2：`must_include` 缺失时的 evidence 不再回显字面短语，改成“答案未覆盖判据「…」要求的关键内容”，exec 没有可粘贴的标签。
- rubric 解析现在拿到用户输入，用于区分夹具令牌与真实任务。

### 新增
- review 生成物落盘：guide 与 judge 每次模型调用都在解析前写入 `<data>/review-artifacts/<session>/`。guide 文件为 `<unix-ms>-guide-a<attempt>.json`，judge 文件为 `<unix-ms>-judge-v<vote>-a<attempt>.json`，保留原始输出与解析错误。
- `review.guide` 增加 `artifact`；`review.verdict` 增加 `attempts`、`votes`、`artifacts`。落盘是 best-effort，不会因磁盘错误拖垮门禁。
- 测试：`review_rubric_rejects_schema_placeholders`、`review_hardcheck_does_not_leak_must_include_literal`，以及 `review_gate_three_roles_retry_and_approve` 的端到端生成物断言。

### 变更
- 判据校验更严：抄 schema 的 guide 会被拒绝并重试，不再生成永远无法满足的判据。
- 插件设置未变，`plugins/review-gate/plugin.json` 仍是 14 个字段；生成物始终记录。

## [1.2.1-rc.3.3] - 2026-11-25

### 修复
- K1：判据先分流。`must_include` / `must_not_include` / `min_points` / `pattern` / `regex` 交给 `review.hardcheck`；只有描述型判据进入 `review.judge`，负向判据通过时不再需要“答案原文”这种不可能存在的证据。
- K2：judge 审批缺项时，把完整验收项 id 清单回注并在同一票内补投；漏一项不再整轮 degraded。
- K3：`bench --json` 增加 `boot_ms_p90` 与 `boot_sample_count`；门禁仍看中位，但尾部可见。
- K4：`metrics.env` 在 Linux 也输出 `mem_load_pct`，并增加 `reliability`；内存 ≥85% 时附 `note`，门禁不转红，只标注读数可信度。

### 新增
- 判据键 `regex` + `min_matches`（1–50，正则最长 200 字符），guide 阶段即编译校验。
- `hardcheck.pattern_engine`（默认 `glob` / `regex`）、`criteria_routing`（默认 `auto` / `hardcheck_only` / `judge_only`）。
- 串行 `judge.votes`（1 或 3）、`majority` 聚合、`early_stop_on_agreement`；温度 base / +0.05 / +0.10 封顶 0.2；峰值模型并发仍为 1。

### 变更
- `plugins/review-gate/plugin.json` 声明 14 个设置字段（原 9 个），TUI 与 WebUI 通用渲染。
- `min_points` 的 `review.warn` 文本改为“按软判据接受…”，`hardcheck_only` 会提示有多少条描述型判据未评估。

## [1.2.1-rc.3.2] - 2026-11-24

### 修复
- J1：`min_points` 不再单独判死一轮。`count_points` 增加中文分号分段、加粗小标题与空行段落识别（取最大值，上限 12）；计数不足改为软判据，引擎打 `review.warn` 并把机器计数提示交给 judge，由 judge 按答案原文裁决。
- J2：review feedback 不再用裸 `c1:` 前缀（exec 会把它抄进答案）。改为 `〔1〕`/`〔2〕` 自然序号，并明确禁止把序号或 `c1/c2` 编号写进答案。
- `featheragent-tui version` / `featheragent-webui version` 改为打印 `FeatherAgent TUI` / `FeatherAgent WebUI`（二进制名不变）。

### 新增
- `bench --json` 的 `metrics.env` 增加环境指纹：采集时间、CPU 数、负载、总/可用内存与 uptime（平台支持时），人类可读输出加一行。门禁阈值不变。

## [1.2.1-rc.3.1] - 2026-11-24

### 修复
- 真实 7B guide 下的 review-gate：guide prompt 给出明确的 JSON schema 与 few-shot；`review.guide` 支持 `feedback`，`review_prepare` 把上一轮 `review.parse_rubric` 的错误回注进重试，而不是原样重发（最多 3 次）。
- `review.parse_rubric` 接受 1–5 条，只带描述的条目标为 `judge_only` 保留，整份清单只需一条硬判据；始终是软判据时以 judge-only 评估并打 `review.warn`，不再整轮 `degraded`。
- `tools/s3_batch.py` t6 现在只允许一次工具调用，并禁止 `tool.shell` / `tool.ping`。

### 新增
- `EventKind::ReviewWarn`（`review.warn`，编号 29），用于判据质量告警与被拒重试。
- `SettingField.options_source` / `allow_custom`；`plugins/review-gate/plugin.json` 把模型字段声明为 `models`，TUI 与 WebUI 通过 `provider.models` 渲染真下拉（仍可手动输入）。
- `tool.confirm_request` 带 `arguments` 与人读 `preview`；TUI 新增权限弹窗，WebUI 弹窗展示完整命令。

### 变更
- TUI 插件页：任意值字段 Enter 即保存整张表单（启停行先翻转再保存）。
- 用户可见产品名统一为 **FeatherAgent**（二进制、子命令、包名与路径不变）。

## [1.2.1-rc.3] - 2026-11-24

### 新增

- `manifest.settings` 插件契约：`root`、`title`、`description` 与 `fields[]`；字段类型限定 `bool|int|number|string|enum`，支持 `default`、区间、选项与可选绝对 `path`。
- `plugin.settings` RPC：返回声明 schema、当前值与默认值；未声明设置项的插件返回业务错误。
- `Config.plugin_settings` 命名空间，作为未声明 `root` 插件的兜底。

### 变更

- 插件设置不再塞进通用设置页：TUI 与 WebUI 都按声明通用渲染任意插件的表单，装新插件不需要改前端。
- 登记表持久化每个插件的设置 schema，并自动补齐本版之前登记的旧条目。
- `config.set` 按声明校验（类型、区间、枚举），首次写入自动补父级；`plugin_settings.<name>` 下未声明的键被拒绝。
- 插件侧边栏与详情标题以插件名为主标签，类别只作次要信息。

### 修复

- H1：判据限制为 2–5 条短语（`must_include` ≤20 字、拒绝整句），且至少一条 `must_include`、至少一条非子串判据（`must_not_include`、`min_points`、`pattern`）；judge 前的硬校验覆盖四类。
- H2：judge 的 evidence 与总结必须是答案中连续 ≥10 字原文；抄提示词与占位词一律拒绝。
- H3：未知工具名返回 `未知工具 … 可用工具：…` 的正常工具错误，不再原样转发成 RPC 失败。
- H4：S3 的 t5 强制调用 `tool.fs.read`、t6 强制只调用一次，S3 口径可复现。

## [1.2.1-rc.2.1] - 2026-11-24

### 修复

- R-01：原生 Ollama 模型发现改走 `/api/tags`，不再请求会 404 的 `/models`；TUI 与 WebUI 的 `provider-ollama` 模型列表恢复。
- R-02：TUI `/settings` 可编辑并保存 review-gate 的启用、轮次、guide/exec/judge 模型与温度；校验温度区间，19 字段表单支持滚动。
- R-03：review rubric 强制字面 `must_include`；内核在 judge 前硬校验，修订反馈带上 `criterion` 与 `check`。
- R-04：judge evidence 少于 10 字视为失败并重试 judge 一次；guide rubric 失败重试一次；review 空响应自动重试一次。
- R-05：`chat.finished.status` 按评审结果返回 `ok`/`unapproved`/`degraded`；新增 `review.status` 与规范化 verdict `items`。

### 变更

- TUI 与 WebUI 的 review 提示建议 guide/judge 使用 ≥7B 模型，4B 可能无法产出合规字面 rubric。

## [1.2.1-rc.2] - 2026-11-24

### 修复

- P0：provider API Key 为空时，空的 Authorization 在 `Content-Length` 之前留下空行，HTTP 头被提前终止，Ollama/OpenAI 兼容请求表现为空 body（`400 missing request body`）。现改为逐行追加头，`Content-Length` 与 body 始终存在。
- P1：review-gate 三角色温度区间与 `enabled` 无关，越界值在写入 `config.json` 前拒绝。
- P2：`serve` 打印最终生效的 model/provider/base URL 及来源；CLI 与 config 冲突时显式 warning。
- embedding 请求使用当前模型，不再回落到 provider 默认模型。

### 新增

- 空 Key 请求体回归测试（`/api/chat`、`/api/embed`）、原始头/body 布局测试、review-gate 区间拒绝测试。

## [1.2.1-rc.1] - 2026-11-24

### Added

- 英文成为文档主语言；中文版统一使用 `.zh-CN.md` 后缀，根目录与核心文档均保留中英双版。
- TUI 与 WebUI 只保留中文界面；已移除 `--lang` 切换与英文界面资源，内核保持语言无关。
- Windows 包同时包含英文测试指南与中文版 `START-HERE.zh-CN.md`。
- Windows 可执行文件图标与 WebUI favicon 改用透明底羽毛图标；`logo-tile.png` 仍作为 Apple touch icon 的不透明底图。
- `review-gate` 循环插件：guide → exec → judge，结构化验收清单/审批结果、必须有 evidence 才通过、1–5 轮修订、review 事件与 WebUI 设置。
- 本地 provider 插件：`provider-llamacpp`、`provider-ninfer`、`provider-lmstudio`、`provider-vllm` 与通用 `provider-openai`，均走 OpenAI 兼容协议。
- 所有 provider 支持可选 Bearer Key，含 Ollama 原生与 OpenAI 兼容路径；TUI/WebUI 可填写，仅存本机。

### Fixed

- R1 启动门禁：热启动改为同一安装根、预热 1 次丢弃、再取 5 次中位；真实 spawn 冷启动单独计量并单设门禁。Windows 插件登记/杀毒尖峰不再干扰 100 ms 热启动线。

### Changed

- `README.md`、`CHANGELOG.md`、`CONTRIBUTING.md`、`SECURITY.md`、`CODE_OF_CONDUCT.md`、架构设计、验证报告、Wire Format、Windows 测试指南与 v1.2.0-rc.1 变更说明默认英文。
- 历史 Windows 复测记录仅保留中文，文件名统一加 `.zh-CN.md`。
- bench 字段新增 `boot_mode`、`boot_samples_ms`、`boot_warmup_discarded_ms`、`cold_spawn_mode`、`cold_spawn_ms_p50` 与分平台冷启动门禁；`spawn_to_socket_ms_min` 保留为兼容别名。

## [1.2.0-rc.1] - 2026-11-23

### Added

- MIT `LICENSE`，以及 `SECURITY.md`、`CONTRIBUTING.md`、`CODE_OF_CONDUCT.md`、本 CHANGELOG。
- 本地 CI `scripts/ci.sh`：Linux fmt/clippy/test/release build；`--with-windows` 追加 Windows x64 交叉编译并检查 `.rsrc` 与 MinGW 运行时 DLL。
- `.github/CODEOWNERS` 与 Pull Request 模板。
- `scripts/version-check.sh`：校验 Cargo、锁文件、5 个插件 manifest 与 WebUI 版本占位一致。
- `scripts/build-linux.sh`：Linux 版本门禁 + `--locked` 构建入口。
- `docs/v1.2.0-rc.1-变更说明.zh-CN.md`；README 增加平台支持矩阵与“已实测 / 未实测”表。

### Fixed

- `web.search` / `web.news` 的 `count` 接受数字字符串（`"5"`）并在内核转发前归一化为整数；schema 仍为 integer。
- TUI 插件页标题不再硬编码 `v1.1.8`，改为 `CARGO_PKG_VERSION`。
- WebUI 顶栏版本改为启动时注入 `{{VERSION}}`，不再硬编码。
- 本地搜索 User-Agent 版本改为 `CARGO_PKG_VERSION`。
- `featheragent --version` 不再误启动内核；TUI/WebUI 新增 `--version`/`--help`。
- 修复 clippy 报告的 16 项代码问题（类型别名、范围循环、`&Path`、`checked_div`、排序等），`cargo clippy -D warnings` 清零。
- 集成测试版本断言改为当前包版本，避免发版时误判。

### Changed

- Windows PE 资源新增 `FileVersion` / `ProductVersion` / MIT 版权，CompanyName 改为 `FeatherAgent contributors`。
- Linux 与 Windows 打包都带 `LICENSE`；Windows 另带 SECURITY/CONTRIBUTING/CODE_OF_CONDUCT/CHANGELOG。
- `Cargo.toml` 增加 `license = "MIT"`、`repository`、`readme`、`authors`、`rust-version = "1.80"`。

### Security

- 扫描全部提交历史与工作区：未发现 `sk-*`、`ghp_*`、AKIA、私钥、`.env` 或大体积二进制泄漏；仅 `target/`、`dist/`、`__pycache__/` 被忽略。

### Verification

- Linux：**86/86** 自动化测试通过（57 lib + 2 plugin + 2 kernel + 3 TUI + 3 WebUI + 2 golden + 17 integration），1 个需公网用例单独通过。
- `cargo fmt --check`、`cargo clippy --all-targets --locked -- -D warnings` 通过。
- Windows x64 交叉编译 5 个 exe；具体哈希见 Release 资产与 manifest。

## [1.1.9] - 2026-11-23

### Fixed

- 项目工作区路径双重嵌套：`tool_base()` 只去掉一层当前项目名前缀。
- `/stop` 提前停止误报 `failed`：取消态发 `finish=interrupted` 空消息，状态统一为 `stopped/cancelled`。
- WebSocket 空闲 15 秒后页面卡 `running`：事件线程超时后继续转发，仅断开连接才退出。
- `tools/s3_batch.py` 适配项目级工作区、唯一 session 与写盘断言。

### Added

- TUI/WebUI 助手消息 Markdown 渲染（标题、列表、引用、代码块、行内样式、链接）。
- WebUI/Windows 羽毛图标（favicon、logo、5 个 exe 资源段）。
- 零 Key 本地联网搜索（必应/DuckDuckGo/Wikipedia/HN/GitHub + 新闻/抓取/联想/状态），默认 `search_provider=local`。
