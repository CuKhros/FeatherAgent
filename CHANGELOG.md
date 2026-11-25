# Changelog

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow semantic versioning. `v1.2.1-rc.3.4` is the current public release candidate and does not replace a stable release.

## [1.2.1-rc.3.4] - 2026-11-25

### Fixed
- L1: the guide no longer copies the JSON schema. Every example value in `GUIDE_SYSTEM` is now an angle-bracket placeholder (`<...>`), the mock-only `REVIEW-FINAL` is gone, and `review.parse_rubric` rejects placeholder text in `criterion` / `check` / `must_include` / `must_not_include`. A `must_include` of `REVIEW-FINAL` / `REVIEW-START` is rejected when it does not appear in the user input. The check is limited to those mock markers so domain terms such as `HTTP` or `JSON` are not rejected.
- L2: a missing `must_include` evidence line no longer echoes the literal phrase. It says the answer does not cover the requirement in the named criterion, so the exec model has no label to paste at the end of the answer.
- The rubric parser now receives the user input, which is needed to tell fixture tokens from real tasks.

### Added
- Review artifacts: every guide and judge model call is written to `<data>/review-artifacts/<session>/` before parsing. Guide files are `<unix-ms>-guide-a<attempt>.json`; judge files are `<unix-ms>-judge-v<vote>-a<attempt>.json`, and each keeps the raw output plus parse error.
- `review.guide` carries `artifact`; `review.verdict` carries `attempts`, `votes`, and `artifacts`. Artifact writing is best-effort and never degrades the gate.
- Tests: `review_rubric_rejects_schema_placeholders`, `review_hardcheck_does_not_leak_must_include_literal`, and end-to-end artifact assertions in `review_gate_three_roles_retry_and_approve`.

### Changed
- Rubric validation is stricter: a guide that copies schema text is rejected and retried instead of producing an impossible criterion.
- No plugin setting changed; `plugins/review-gate/plugin.json` still declares 14 fields, and artifacts are always recorded.

## [1.2.1-rc.3.3] - 2026-11-25

### Fixed
- K1: criteria are routed before the judge. `must_include` / `must_not_include` / `min_points` / `pattern` / `regex` are settled by `review.hardcheck`; only description items go to `review.judge`, so a passing negative criterion no longer needs “answer text” that cannot exist.
- K2: an incomplete judge verdict is reinjected with the full item-id list and retried inside the same vote; one omitted item no longer degrades the whole round.
- K3: `bench --json` adds `boot_ms_p90` and `boot_sample_count`; the gate still uses the median, but the tail is visible.
- K4: `metrics.env` reports `mem_load_pct` on Linux too, plus `reliability` and a `note` when memory load is ≥85%; gates stay green, the reading is annotated.

### Added
- `review` criterion `regex` with `min_matches` (1–50, 200-character pattern limit), validated during guide.
- `hardcheck.pattern_engine` (`glob` default / `regex`), `criteria_routing` (`auto` default / `hardcheck_only` / `judge_only`).
- Serial `judge.votes` (1 or 3), `majority` aggregation, `early_stop_on_agreement`; temperatures base / +0.05 / +0.10 capped at 0.2; peak model concurrency remains 1.

### Changed
- `plugins/review-gate/plugin.json` declares 14 settings fields (was 9); TUI and WebUI render them generically.
- `review.warn` text for `min_points` now says the item is accepted as a soft criterion, and `hardcheck_only` warns how many description items were not evaluated.

## [1.2.1-rc.3.2] - 2026-11-24

### Fixed
- J1: `min_points` no longer kills a round by itself. `count_points` now also recognises Chinese semicolon-separated clauses, bold sub-headings and blank-line paragraphs (taking the maximum, capped at 12); a shortfall is returned as a soft item, the engine emits `review.warn` and passes a machine-count note to the judge, which decides from the answer itself.
- J2: review feedback no longer uses bare `c1:` prefixes that the exec model copied into its answer. Items are labelled `〔1〕`/`〔2〕` and the prompt explicitly forbids writing those numbers or `c1/c2`-style ids into the answer.
- `featheragent-tui version` / `featheragent-webui version` now print `FeatherAgent TUI` / `FeatherAgent WebUI` (binary names unchanged).

### Added
- Bench environment fingerprint in `bench --json` under `metrics.env`: capture time, CPU count, load average, total/available memory and uptime where the platform exposes them, plus one human-readable line. Gates are unchanged.

## [1.2.1-rc.3.1] - 2026-11-24

### Fixed
- Review-gate on a real 7B guide: the guide prompt now ships a literal JSON schema and few-shot; `review.guide` accepts `feedback` and `review_prepare` reinjects the previous `review.parse_rubric` error instead of resending the same prompt (3 attempts).
- `review.parse_rubric` accepts 1–5 items, keeps description-only items as `judge_only`, and only requires one hard criterion across the rubric; an always-soft rubric ends in judge-only evaluation with `review.warn` instead of `degraded`.
- `tools/s3_batch.py` t6 now allows exactly one tool call and forbids `tool.shell` / `tool.ping`.

### Added
- `EventKind::ReviewWarn` (`review.warn`, code 29) for rubric quality warnings and rejected attempts.
- `SettingField.options_source` / `allow_custom`; `plugins/review-gate/plugin.json` declares model fields as `models`, so TUI and WebUI show a real dropdown fed by `provider.models` (manual entry still allowed).
- `tool.confirm_request` carries `arguments` and a human-readable `preview`; TUI gets a permission popup and WebUI shows the command in its modal.

### Changed
- TUI plugin page: Enter on any value field saves the form (enable row toggles then saves).
- User-visible product name is **FeatherAgent** (binaries, subcommands, package names and paths unchanged).

## [1.2.1-rc.3] - 2026-11-24

### Added

- `manifest.settings` plugin contract: `root`, `title`, `description`, and `fields[]` limited to `bool|int|number|string|enum` with `default`, range, options, and optional absolute `path`.
- `plugin.settings` RPC returning the declared schema plus current values and defaults; undeclared plugins return a business error.
- `Config.plugin_settings` namespace for plugins that declare no `root`.

### Changed

- Plugin settings are no longer hard-coded into the general settings page: TUI and WebUI render every plugin's declared form generically, so adding a plugin needs no front-end change.
- The registry persists each plugin's settings schema and backfills entries registered before this release.
- `config.set` validates declared paths (type, range, enum) and creates missing parents on first write; undeclared `plugin_settings.<name>` keys are rejected.
- Plugin sidebars and detail headers show the plugin name, with category as secondary text.

### Fixed

- H1: rubrics are now 2–5 phrase-only items (`must_include` ≤20 characters, no sentence-like phrases) with at least one `must_include` and at least one non-substring criterion (`must_not_include`, `min_points`, `pattern`); the hardcheck covers all four kinds before the judge runs.
- H2: judge evidence and the verdict summary must be a verbatim contiguous ≥10-character window of the answer; prompt echoes and placeholders are rejected.
- H3: unknown tool names return `未知工具 … 可用工具：…` as a normal tool error instead of a forwarded RPC failure.
- H4: S3 t5 requires calling `tool.fs.read` and t6 requires exactly one call, making the S3 rate reproducible.

## [1.2.1-rc.2.1] - 2026-11-24

### Fixed

- R-01: native Ollama model discovery now probes `/api/tags` instead of `/models`; TUI and WebUI model lists work again for `provider-ollama`.
- R-02: TUI `/settings` now edits and saves all review-gate fields (enabled, rounds, guide/exec/judge model and temperature), validates temperature ranges, and scrolls the 19-field form.
- R-03: review rubrics require a literal `must_include`; the kernel hard-checks it before the judge, and revision feedback now carries `criterion` and `check`.
- R-04: judge evidence shorter than 10 characters is rejected and the judge is retried once; guide rubrics are retried once; empty review responses retry once.
- R-05: `chat.finished.status` is now `ok`, `unapproved`, or `degraded` according to the review result; `review.status` and normalized verdict `items` are exposed.

### Changed

- TUI and WebUI review hints recommend a ≥7B guide/judge model because 4B models may not produce a compliant literal rubric.

## [1.2.1-rc.2] - 2026-11-24

### Fixed

- P0: an empty provider API key inserted a blank line before `Content-Length`, terminating HTTP headers early. Ollama and OpenAI-compatible requests went out with no visible body (`400 missing request body`). Headers are now appended line by line; body and `Content-Length` are always present.
- P1: review-gate guide/exec/judge temperature ranges are enforced regardless of `enabled`, and invalid values are rejected before `config.json` is written.
- P2: `serve` prints the effective model/provider/base URL and their source; a CLI/config conflict emits an explicit warning.
- Embedding requests now carry the active model instead of the provider default.

### Added

- Regression tests for empty-key request bodies (`/api/chat` and `/api/embed`), raw header/body layout, and review-gate range rejection.

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
