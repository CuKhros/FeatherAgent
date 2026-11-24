# Changelog

格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本遵循语义化版本。`v1.2.1-rc.2.1` 是当前公开仓库候选版，尚未替代稳定版。

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
