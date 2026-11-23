# Changelog

格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本遵循语义化版本。`v1.2.0-rc.1` 是公开仓库候选版，尚未替代稳定版。

## [1.2.0-rc.1] - 2026-11-23

### Added

- MIT `LICENSE`，以及 `SECURITY.md`、`CONTRIBUTING.md`、`CODE_OF_CONDUCT.md`、本 CHANGELOG。
- 本地 CI `scripts/ci.sh`：Linux fmt/clippy/test/release build；`--with-windows` 追加 Windows x64 交叉编译并检查 `.rsrc` 与 MinGW 运行时 DLL。
- `.github/CODEOWNERS` 与 Pull Request 模板。
- `scripts/version-check.sh`：校验 Cargo、锁文件、5 个插件 manifest 与 WebUI 版本占位一致。
- `scripts/build-linux.sh`：Linux 版本门禁 + `--locked` 构建入口。
- `docs/v1.2.0-rc.1-变更说明.md`；README 增加平台支持矩阵与“已实测 / 未实测”表。

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
