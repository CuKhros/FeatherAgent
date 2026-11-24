# Changelog

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow semantic versioning. `v1.2.1-rc.1` is the current public release candidate and does not replace a stable release.

## [1.2.1-rc.1] - 2026-11-24

### Added

- English is now the primary documentation language. Chinese editions use the `.zh-CN.md` suffix; root and core documents have both editions.
- TUI and WebUI are **Chinese-only**. The `--lang` switch and the English UI asset set were removed in a follow-up; the kernel stays language-neutral.
- WebUI package documentation includes both the English test guide and the Chinese edition.
- The Windows executable icon and the WebUI favicon now use the transparent-background feather icon. `logo-tile.png` remains the opaque tile for Apple touch icons.
- `review-gate` loop plugin: guide → exec → judge, structured rubric/verdict, evidence-required approval, 1–5 revision rounds, review events, and WebUI settings.
- Local provider plugins: `provider-llamacpp`, `provider-ninfer`, `provider-lmstudio`, `provider-vllm`, and generic `provider-openai`, all OpenAI-compatible.
- Optional Bearer API key for all providers, including Ollama native and OpenAI-compatible paths; TUI and WebUI fields store it locally.

### Fixed

- R1 startup gate: warm boot now uses one install root with one discarded warm-up and five samples; cold spawn is measured separately with its own gate. This removes the Windows plugin-registration/AV spikes from the 100 ms warm line.

### Changed

- `README.md`, `CHANGELOG.md`, `CONTRIBUTING.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md`, architecture, verification report, Wire Format, Windows test guide, and v1.2.0-rc.1 change note are English by default.
- Historical Windows retest notes remain Chinese-only and are renamed with `.zh-CN.md`.
- Bench schema now exposes `boot_mode`, `boot_samples_ms`, `boot_warmup_discarded_ms`, `cold_spawn_mode`, `cold_spawn_ms_p50`, and platform-specific cold gates. `spawn_to_socket_ms_min` remains as a compatibility alias.

## [1.2.0-rc.1] - 2026-11-23

### Added

- MIT `LICENSE`, plus `SECURITY.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, and this CHANGELOG.
- Local CI `scripts/ci.sh`: Linux fmt/clippy/test/release build; `--with-windows` adds Windows x64 cross-compilation and checks `.rsrc` and MinGW runtime DLLs.
- `.github/CODEOWNERS` and a Pull Request template.
- `scripts/version-check.sh`: validates Cargo, lock file, five plugin manifests, and the WebUI version placeholder.
- `scripts/build-linux.sh`: Linux version gate + `--locked` build entry point.
- `docs/v1.2.0-rc.1-Changes.md`; README adds a platform support matrix and a tested/untested table.

### Fixed

- `web.search` / `web.news` `count` accepts numeric strings (`"5"`) and normalizes them to integers before kernel forwarding; the schema remains integer.
- TUI plugin-page title is no longer hard-coded to `v1.1.8`; it uses `CARGO_PKG_VERSION`.
- WebUI header version is injected as `{{VERSION}}` at startup instead of being hard-coded.
- Local-search User-Agent version uses `CARGO_PKG_VERSION`.
- `featheragent --version` no longer starts the kernel by mistake; TUI/WebUI gain `--version` / `--help`.
- Fixed 16 clippy findings (type aliases, range loops, `&Path`, `checked_div`, sorting, and more), bringing `cargo clippy -D warnings` to zero.
- Integration-test version assertions use the current package version, avoiding false failures at release time.

### Changed

- Windows PE resources gain `FileVersion` / `ProductVersion` / MIT copyright; CompanyName becomes `FeatherAgent contributors`.
- Linux and Windows packages both include `LICENSE`; Windows also includes SECURITY/CONTRIBUTING/CODE_OF_CONDUCT/CHANGELOG.
- `Cargo.toml` gains `license = "MIT"`, `repository`, `readme`, `authors`, and `rust-version = "1.80"`.

### Security

- Scanned all commit history and the working tree: no `sk-*`, `ghp_*`, AKIA, private keys, `.env`, or large-binary leaks; only `target/`, `dist/`, and `__pycache__/` are ignored.

### Verification

- Linux: **86/86** automation tests pass (57 lib + 2 plugin + 2 kernel + 3 TUI + 3 WebUI + 2 golden + 17 integration); one network-dependent case passes separately.
- `cargo fmt --check` and `cargo clippy --all-targets --locked -- -D warnings` pass.
- Windows x64 cross-compiles five exes; hashes are in the release assets and manifest.

## [1.1.9] - 2026-11-23

### Fixed

- Project workspace path double nesting: `tool_base()` strips only one current-project prefix.
- `/stop` reported `failed` after an early stop: cancellation emits an empty `finish=interrupted` message and maps state to `stopped/cancelled`.
- WebSocket page stuck at `running` after 15 seconds idle: the event thread continues forwarding after timeouts and exits only when the connection closes.
- `tools/s3_batch.py` adapted to project-level workspaces, unique sessions, and disk-write assertions.

### Added

- TUI/WebUI assistant Markdown rendering (headings, lists, quotes, code blocks, inline styles, links).
- WebUI/Windows feather icons (favicon, logo, resources in all five exes).
- Keyless local web search (Bing/DuckDuckGo/Wikipedia/HN/GitHub plus news/fetch/suggest/status), `search_provider=local` by default.
