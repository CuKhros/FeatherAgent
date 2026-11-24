# FeatherAgent

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust 1.80+](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org/)

A context engine for **1B–14B local models**, plus a **tiny, zero-runtime-dependency** portable agent kernel.

**Why it exists:** to take the repetitive, mechanical parts of an agent's work off the orchestrator's hands. The orchestrator keeps the steps that need judgment and hands the high-volume, low-risk chores to a 1B–14B model on the same machine — no cloud round trip, no per-token bill, no context the frontier model has to carry.

> English is the primary documentation language. Chinese editions use the `.zh-CN.md` suffix, e.g. [README.zh-CN.md](README.zh-CN.md).

## Why FeatherAgent

1. **Context engineering for small models** — four-layer compression, budget accounting, tolerant parsing, KV-cache-friendly prefixes.
2. **A very small kernel** — one Rust binary; target **≤ 8 MB**, idle RSS **≤ 15 MB**, warm start **< 20 ms**.
3. **Portability** — no Node, Python, or JVM runtime. Install root, data directory, and workspace can live in separate locations.

"Everything is a plugin" is the implementation strategy, not a headline feature.

## Architecture

Three processes, two contracts, one kernel:

```text
Frontend layer (TUI in core package / standalone WebUI / third-party clients)
   │  ① Frontend Contract (IDL v1, UDS / named pipe; WebSocket bridge lives in WebUI package)
Kernel (one Rust binary ≤ 8 MB): orchestration · append-only log · plugin runtime · governance · contract
   │  ② Plugin Contract (IDL v1, stdio JSON-RPC / MCP-compatible)
Plugin process layer: context-engine · storage/index · provider-* · tool-* · bridge-*
   │
Local inference runtime (external): Ollama / llama.cpp / NInfer / LM Studio / vLLM / OpenAI-compatible servers
```

Four kernel invariants: the kernel does not understand agents · every capability is a plugin · everything is traceable (append-only event stream) · **the kernel owns no business storage**.

## Key decisions (v1.1)

| Area | Decision |
|---|---|
| Kernel language | **Rust** (flipped from Go in v1.0; Go remains a fallback option, plugins and frontends are unchanged) |
| Event log | **Custom append-only format** (segments + CRC32C + `seq` ordering, wall clock `CLOCK_REALTIME`), no SQLite |
| Index / vectors | Moved into a **separate storage plugin process** (SQLite + FTS5 by default; redb / tantivy / external service optional) |
| Plugin sandbox | Out-of-process RPC (real security boundary) + **WASM (wasmi)**; goja / JS plugins removed |
| Frontends | TUI (core package) + **WebUI** (separately distributed: web assets + `featheragent-webui` bridge + Chromium `--app` host). A native GUI is a future plugin |
| Distribution | Three artifacts: `featheragent-core` / `featheragent-webui` / `featheragent-plugins` |
| Roadmap | **Phases + gates**, no dates: Phase 0 three spikes → 1 kernel → 2 context engine → 3 storage and security → 4 WebUI → 5 distribution → 6 native GUI plugin |

## Size model (three separate metrics)

| Metric | Target | Nature |
|---|---|---|
| Out-of-box gate | soft ≤ 100 MiB / hard ≤ 1 GiB | hard assertion in CI |
| Steady-state usage | 1–2.5 GiB | monitored metric |
| User quota | 4 GiB default | visible in UI + one-click cleanup |

## Non-goals

No built-in inference engine, no model weights, no cloud account, no telemetry, no mobile client.

## Platform support matrix

> This is the **current verification state**, not a promise about future platforms. Untested platforms are not part of the release gate.

| Platform | Arch | Build path | Release form | Current state |
|---|---|---|---|---|
| Linux x64 | x86_64 | native `cargo` / `scripts/build-linux.sh` | three tar.gz artifacts | **tested** (automation, TUI PTY, WebUI, mock/Ollama integration) |
| Windows x64 | x86_64 | mingw-w64 cross-compile / `scripts/build-windows.sh` | one zip with five exes | **tested** (v1.1.9 real hardware; v1.2.1-rc.3.2 pending retest) |
| Linux arm64 | aarch64 | no build script | none | not supported, untested |
| Windows arm64 | aarch64 | no build script | none | not supported, untested |
| Windows x86 | i686 | no build script | none | not supported, untested |
| macOS x64 / arm64 | x86_64 / aarch64 | no build script | none | not supported, untested |

## Tested / not tested

| Capability | Linux x64 | Windows x64 | Evidence / notes |
|---|---|---|---|
| Kernel startup, event log, plugin RPC | ✅ automation | ✅ v1.1.9 real hardware | Linux `cargo test`; Windows serve/chat |
| Seven benchmark gates | ✅ 7/7 (v1.2.1) | ✅ 6/6 (v1.1.9) | v1.2.1 adds a separate warm/cold spawn contract; Windows 7/7 retest pending |
| `review-gate` loop plugin | ✅ integration test | ⚠️ not tested | guide/exec/judge retry, events, WebUI settings |
| Local providers + Bearer key | ✅ mock HTTP integration | ⚠️ not tested | llama.cpp/NInfer/LM Studio/vLLM/OpenAI-compatible; real servers not connected |
| Project workspace / `.featheagent` | ✅ automation | ✅ v1.1.9 real hardware | path de-duplication, S3 writes |
| TUI interaction | ✅ PTY | ✅ v1.1.9 real hardware | Markdown, `/stop`, `/settings` |
| WebUI | ✅ browser | ✅ v1.1.9 real hardware | Markdown, icon, idle WebSocket retest (Linux) |
| Keyless local web search | ✅ tested | ✅ v1.1.9 real hardware | Bing/Baidu/China News fallback, `web.search` second pass |
| S3 tool levels | ✅ 4/5 | ✅ 4/5 (v1.1.9) | zero real failures; script still needs real Ollama |
| In-turn steer / `/compact` | ⚠️ unit-tested, not two-turn tested | ⚠️ scenario not built | explicitly unverified in v1.1.9 report |
| DeepSeek paid search | ❌ not called | ❌ not called | requires an API key, billed per successful request |
| High DPI / Chinese rendering | ⚠️ normal DPI | ❌ untested | Windows high-DPI retest pending |
| Network drive / sync drive / 24h soak | ❌ untested | ❌ untested | not used as a release conclusion |
| macOS / arm64 | ❌ unsupported | ❌ unsupported | no artifacts |

## Quick start (Linux x64)

Requires Rust 1.80+; no other runtime dependency.

```bash
scripts/build-linux.sh                                  # version gate + --locked build
./target/release/featheragent serve --root .            # kernel + UDS
./target/release/featheragent-tui --root .              # TUI (second terminal)
./target/release/featheragent-webui --root . --app      # WebUI (Chromium --app window)
```

Factory plugins live in `plugins/`: `provider-mock`, `provider-ollama`, `provider-llamacpp`, `provider-ninfer`, `provider-lmstudio`, `provider-vllm`, generic `provider-openai`, `review-gate`, `tool`, `storage`, and `search`. `serve` scans `<install root>/plugins` and defaults to `provider-mock`. To use real Ollama: start `ollama serve`, then `serve --provider provider-ollama --model qwen2.5:7b`. Ollama native `/api/*` and `/v1` base URLs are both accepted; llama.cpp/NInfer/LM Studio/vLLM use their OpenAI-compatible `/v1` endpoints and the base URL is normalized automatically.

The TUI and WebUI are **Chinese-only**; there is no `--lang` switch and no English UI asset set in the binaries. The kernel itself is language-neutral, while the documentation remains available in English and Chinese.

The WebUI **Settings** page and TUI `/settings` switch provider, model, temperature, context parameters, workspace, and an optional Bearer API key. The key is sent only to the selected provider and stored locally in `config.json`. The WebUI **Plugins** page and TUI `/plugins` install/enable/disable plugins; each registered plugin gets a settings-sidebar page. The model dropdown lists everything returned by `provider.models`; saving applies it through `config.set`. `serve` prints a banner and writes `runtime/ipc/starting` / `ready`; if startup is slow, `chat` reports "kernel not ready (starting)".

Package the three Linux artifacts with `scripts/package.sh`; output is `dist/*.tar.gz` plus `sha256` and a manifest. The kernel size gate (≤ 8 MiB) is asserted by the script.

At runtime the install root gets `data/`, `workspace/`, and `runtime/`. Use `--data/--workspace/--runtime` to relocate them, or point `workspace_root` outside the install directory in Settings (restart `serve` to apply). `workspace_root` is a **project container**: each direct child directory is a workspace with its own `.featheagent/` (`worklog.md` / `memory.md` / `README.md`). The kernel appends real UTC time, goal, and tools to worklog at turn end; models never write the timestamp. During a running turn, `/stop` aborts and preserves context, plain text becomes `chat.inject` steering, and `/compact` compresses old context. In the TUI, ↑↓ scrolls the conversation and Esc returns to the session list; ←/→/Home/End edit input; CJK cursor width is display-width aware. Local web search is the default `search` plugin (`search_provider=local`) and needs no API key; Settings can switch to DeepSeek or disable search.

## Quick start (Windows x64 test package)

Unzip `FeatherAgent-1.2.1-rc.3.2-windows-x64.zip` into a writable directory (not `Program Files`), then:

```powershell
.\featheragent.exe help                       # command table
.\featheragent.exe serve --root .
.\featheragent-tui.exe --root .              # second terminal
.\featheragent-webui.exe --root . --app        # finds Chrome/Edge
.\featheragent.exe chat --root . --text "hello"   # script channel without TUI/WebUI
```

The five exes depend only on Windows system DLLs (no VC++ runtime, no MinGW runtime). `serve` prints a banner immediately; first launch can take 1–2 minutes while antivirus scans plugin binaries and prints `[init]` progress. If `runtime\ipc\ready` is absent, `chat` reports "kernel not ready (starting)" rather than a channel failure. Build: `scripts/build-windows.sh` (mingw-w64 + `x86_64-pc-windows-gnu` + static CRT); package: `scripts/package-windows.sh`. See [Windows Test Guide](docs/Windows-Test-Guide.md) ([中文](docs/Windows测试说明.zh-CN.md)).

## Platform gate differences (v1.2.1-rc.3.2)

| Gate | Linux | Windows | Notes |
|---|---|---|---|
| Warm in-process startup P50 | < 20 ms | **< 100 ms** | same install root, 1 warm-up discarded + 5 samples |
| Cold spawn → socket P50 | < 100 ms | **< 250 ms** | real `serve` child process; Windows includes DLL load and real-time protection |
| Event write P95 | < 100 µs | **< 10 ms** | NTFS `FlushFileBuffers` + real-time protection; environmental, one order of magnitude |
| Kernel size | ≤ 8 MB | ≤ 8 MB | shared |
| Idle RSS | ≤ 15 MB | ≤ 15 MB | shared |
| Context assembly P95 | < 30 ms | < 30 ms | shared |
| RPC P95 | < 1 ms | < 1 ms | shared |

`bench --json` returns `platform`, `boot_mode`, `boot_samples_ms`, `cold_spawn_ms_p50`, `boot_limit_ms`, `cold_spawn_limit_ms`, and `write_limit_us`; Windows is not hard-coded as a failure.

## Contributing and license

- Contributing: [CONTRIBUTING.md](CONTRIBUTING.md) ([中文](CONTRIBUTING.zh-CN.md))
- Security: [SECURITY.md](SECURITY.md) ([中文](SECURITY.zh-CN.md))
- Code of Conduct: [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) ([中文](CODE_OF_CONDUCT.zh-CN.md))
- Branch protection: [main rules](.github/BRANCH_PROTECTION.md)
- Changelog: [CHANGELOG.md](CHANGELOG.md) ([中文](CHANGELOG.zh-CN.md))
- Local CI: [scripts/ci.sh](scripts/ci.sh) (`--with-windows` adds cross-compilation)
- License: [MIT](LICENSE)

## Documentation

- **[Architecture Design v1.1](docs/Architecture-Design-v1.1.md)** — full design: v1.0 → v1.1 changes, erratum R1, technology inventory, roadmap, risks ([中文](docs/FeatherAgent架构设计文档-v1.1.zh-CN.md))
- **[Verification Report](docs/Verification-Report.md)** — test matrix, measured gate numbers, fixed defects, untested items and deviations ([中文](docs/验证报告.zh-CN.md))
- **[Frontend Contract · Wire Format](docs/Wire-Format.md)** — 4-byte little-endian length + MsgPack/JSON, endpoint discovery, handshake order, error codes ([中文](docs/前端契约-WireFormat.zh-CN.md))
- **[v1.2.1-rc.3.2 Changes](docs/v1.2.1-rc.3.2-Changes.md)** — `min_points` soft fallback with `review.warn` + judge review, Chinese semicolon/bold-heading point counting, feedback labels `〔1〕` instead of bare `c1:` ids, bench `metrics.env` fingerprint ([中文](docs/v1.2.1-rc.3.2-变更说明.zh-CN.md))
- **[v1.2.1-rc.3.1 Changes](docs/v1.2.1-rc.3.1-Changes.md)** — guide schema + error reinjection + tolerant judge-only rubric with `review.warn`, model-name dropdowns from `provider.models`, Enter-saves-anywhere on the TUI plugin page, full command in permission prompts, display name **FeatherAgent** ([中文](docs/v1.2.1-rc.3.1-变更说明.zh-CN.md))
- **[v1.2.1-rc.3 Changes](docs/v1.2.1-rc.3-Changes.md)** — plugin-declared settings (`manifest.settings` + `plugin.settings`), generic TUI/WebUI forms, phrase-only rubrics with four criterion kinds, answer-verbatim judge evidence, unknown-tool correction ([中文](docs/v1.2.1-rc.3-变更说明.zh-CN.md))
- **[v1.2.1-rc.2.1 Changes](docs/v1.2.1-rc.2.1-Changes.md)** — Ollama `/api/tags` model list, TUI review-gate settings, literal rubric/hardcheck, judge retry, `chat.finished` status ([中文](docs/v1.2.1-rc.2.1-变更说明.zh-CN.md))
- **[v1.2.1-rc.2 Changes](docs/v1.2.1-rc.2-Changes.md)** — provider empty-body fix, review temperature validation, effective-config banner ([中文](docs/v1.2.1-rc.2-变更说明.zh-CN.md))
- **[v1.2.1-rc.1 Changes](docs/v1.2.1-rc.1-Changes.md)** — R1 warm/cold split, review-gate, local providers, API keys, tests ([中文](docs/v1.2.1-rc.1-变更说明.zh-CN.md))
- **[v1.2.0-rc.1 Changes](docs/v1.2.0-rc.1-Changes.md)** — Q1/Q2, dual-platform version/build alignment, MIT and public-repository materials, platform matrix ([中文](docs/v1.2.0-rc.1-变更说明.zh-CN.md))

Historical Windows retest notes are Chinese-only and use the `.zh-CN.md` suffix:

- [v1.1.9](docs/v1.1.9-Windows修复说明.zh-CN.md), [v1.1.8](docs/v1.1.8-Windows修复说明.zh-CN.md), [v1.1.7](docs/v1.1.7-Windows修复说明.zh-CN.md), [v1.1.6](docs/v1.1.6-Windows修复说明.zh-CN.md), [v1.1.5](docs/v1.1.5-Windows修复说明.zh-CN.md), [v1.1.4](docs/v1.1.4-Windows修复说明.zh-CN.md), [v1.1.3](docs/v1.1.3-Windows修复说明.zh-CN.md), [v1.1.2](docs/v1.1.2-Windows修复说明.zh-CN.md)

> Every target number in the docs must be measured through a spike or phase gate before it is written down; untested numbers are not published.

## Status

`v1.2.1-rc.3.2` fixes the three findings from the rc.3.1 real-hardware retest without touching the architecture: `min_points` is no longer a hard rejection but a soft item handed to the judge (with a broader Chinese-aware point count and a `review.warn`), review feedback uses `〔1〕` labels instead of the bare `c1:` prefixes the exec model copied into answers, and `bench --json` carries a `metrics.env` fingerprint so numbers can be compared across runs.

`v1.2.1-rc.3.1` keeps the rc.3 architecture and fixes what the real-hardware retest found: the guide now gets a literal JSON schema plus a few-shot, its validation error is reinjected into retries instead of resending the same prompt, and a rubric with only descriptions is retried once and then evaluated judge-only with a `review.warn` event rather than degrading the whole round. Plugin settings gained `options_source`, so model fields show a real dropdown fed by the local model service while still accepting a typed name; the TUI plugin page saves on Enter from any field; permission prompts now show the full command/arguments on both front ends; and the product display is **FeatherAgent** (binaries and paths unchanged).

`v1.2.1-rc.3` made plugin settings a declaration: a plugin states its settings schema in `plugins/<name>/plugin.json` (`manifest.settings`: root, title, description, and `bool|int|number|string|enum` fields with range/options), the kernel relays it through the `plugin.settings` RPC and validates writes in `config.set`, and the TUI and WebUI render any plugin's form generically. It also closed the review-gate and S3 findings H1–H4 from the rc.2.1 retest: phrase-only rubrics with four criterion kinds and a pre-judge hardcheck, judge evidence that must be a verbatim ≥10-character window of the answer, an explicit “unknown tool + available tools” correction, and S3 t5/t6 prompts that ask for a single tool call.

Linux automation has **116 tests: 115 passed, 1 ignored by default** (62 lib, 16 plugin, 2 kernel, 3 TUI, 3 WebUI, 2 golden, 27 integration); the ignored network-dependent test passes when run separately. `fmt` and `clippy -D warnings` pass. Settings pages were verified on both front ends: TUI through a PTY (the review-gate page opens with the model hint “本地模型 1 个；←/→ 选择”, and Enter on a value row persisted `max_rounds` 2→3 into `config.json`), and the WebUI bridge over WebSocket (`plugin.settings` returns the three model fields with `options_source:"models"`, and a `tool.shell` confirm request arrived with `arguments` + a `$ echo hi` preview that could be denied).

Windows x64 has gone through nine real-hardware rounds from v1.1.3 to v1.1.9, plus the rc.2 retest that verified the provider body, temperature rejection, banner, R1 gates, and review-gate mechanism. The v1.2.1-rc.3.2 Windows package is cross-compiled and its automation runs only on Linux; the rc.3.1 retest confirmed that the review gate now rejects as well as approves (1/3 approved, 2/3 unapproved; guide `attempts:1`; answer-verbatim evidence), and that run also exposed J1/J2/J3, which this candidate fixes. J1/J2/J3 plus the previous model dropdowns, Enter-to-save and permission popup still need a real-hardware retest. Wine is only a startup smoke test; boot/RSS numbers under Wine are not Windows conclusions.
