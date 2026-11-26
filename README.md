# FeatherAgent

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust 1.80+](https://img.shields.io/badge/rust-1.80%2B-orange.svg)](https://www.rust-lang.org/)

A context engine for **1B–14B local models**, plus a **tiny, zero-runtime-dependency** portable agent kernel.

> **Built primarily by an AI agent.** The code, tests, documentation and release flow are mainly produced and maintained by an autonomous agent; human involvement focuses on direction, acceptance and real-hardware verification.

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
| Windows x64 | x86_64 | mingw-w64 cross-compile / `scripts/build-windows.sh` | one zip with ten exes (Chinese + English `-en`) | **tested** (v1.1.9 real hardware; rc.4.2/rc.4.3 retested; v1.2.1-rc.4.4 package pending retest) |
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

The Chinese executables are the default. Every top-level binary also has an English edition selected by the `-en` filename suffix (`featheragent-en`, `featheragent-tui-en`, `featheragent-webui-en`, `feather-plugin-en`, `feather-storage-en`); there is no `--lang` switch. The English TUI and WebUI carry their own runtime strings and assets, the plugin/storage `-en` builds are protocol-only aliases, and the Chinese editions keep their previous behaviour. The kernel itself is language-neutral, while the documentation remains available in English and Chinese.

The WebUI **Settings** page and TUI `/settings` switch provider, model, temperature, context parameters, workspace, and an optional Bearer API key. The key is sent only to the selected provider and stored locally in `config.json`. The WebUI **Plugins** page and TUI `/plugins` install/enable/disable plugins; each registered plugin gets a settings-sidebar page. The model dropdown lists everything returned by `provider.models`; saving applies it through `config.set`. `serve` prints a banner and writes `runtime/ipc/starting` / `ready`; if startup is slow, `chat` reports "kernel not ready (starting)".

**File attachments.** WebUI has a `+` button that opens the native file picker; TUI accepts `@<path>` (quote paths with spaces as `@"..."`). The front end verifies that the file exists, then calls `file.prepare` / `file.attach`; the kernel rechecks existence and, when the file is outside the install root, copies it into `workspace/default/uploads/`. The path sent to the model is always the kernel-returned path, not the browser's or the original outside path. Uploaded bytes are capped at 8 MiB and travel over HTTP RPC (base64), with a 12 MiB request limit. Search/news/fetch results are also written to `workspace/<project>/.featheagent/search/<timestamp>-<tool>.json`, and the tool result carries an `artifact` path.

Package the three Linux artifacts with `scripts/package.sh`; output is `dist/*.tar.gz` plus `sha256` and a manifest. The kernel size gate (≤ 8 MiB) is asserted by the script.

At runtime the install root gets `data/`, `workspace/`, and `runtime/`. Use `--data/--workspace/--runtime` to relocate them, or point `workspace_root` outside the install directory in Settings (restart `serve` to apply). `workspace_root` is a **project container**: each direct child directory is a workspace with its own `.featheagent/` (`worklog.md` / `memory.md` / `README.md`). The kernel appends real UTC time, goal, and tools to worklog at turn end; models never write the timestamp. During a running turn, `/stop` aborts and preserves context, plain text becomes `chat.inject` steering, and `/compact` compresses old context. In the TUI, ↑↓ scrolls the conversation and Esc returns to the session list; ←/→/Home/End edit input; CJK cursor width is display-width aware. Local web search is the default `search` plugin (`search_provider=local`) and needs no API key; Settings can switch to DeepSeek or disable search.

## Quick start (Windows x64 test package)

Unzip `FeatherAgent-1.2.1-rc.4.4-windows-x64.zip` into a writable directory (not `Program Files`), then:

```powershell
.\featheragent.exe help                       # Chinese command table
.\featheragent-en.exe help                    # English command table
.\featheragent.exe serve --root .
.\featheragent-tui.exe --root .              # Chinese TUI (second terminal)
.\featheragent-tui-en.exe --root .           # English TUI
.\featheragent-webui.exe --root . --app        # Chinese WebUI (finds Chrome/Edge)
.\featheragent-webui-en.exe --root . --app     # English WebUI
.\featheragent.exe chat --root . --text "hello"   # script channel without TUI/WebUI
```

The ten exes depend only on Windows system DLLs (no VC++ runtime, no MinGW runtime). `serve` prints a banner immediately; first launch can take 1–2 minutes while antivirus scans plugin binaries and prints `[init]` progress. If `runtime\ipc\ready` is absent, `chat` reports "kernel not ready (starting)" rather than a channel failure. Build: `scripts/build-windows.sh` (mingw-w64 + `x86_64-pc-windows-gnu` + static CRT); package: `scripts/package-windows.sh`. See [Windows Test Guide](docs/Windows-Test-Guide.md) ([中文](docs/Windows测试说明.zh-CN.md)).

## Platform gate differences (v1.2.1-rc.4.4)

| Gate | Linux | Windows | Notes |
|---|---|---|---|
| Warm in-process startup P50 | < 20 ms | **< 100 ms** | same install root, 1 warm-up discarded + 5 samples |
| Cold spawn → socket P50 | < 100 ms | **< 250 ms** | real `serve` child process; Windows includes DLL load and real-time protection |
| Event write P95 | < 100 µs | **< 10 ms** | NTFS `FlushFileBuffers` + real-time protection; environmental, one order of magnitude |
| Kernel size | ≤ 8 MB | ≤ 8 MB | shared |
| Idle RSS | ≤ 15 MB | ≤ 15 MB | shared |
| Context assembly P95 | < 30 ms | < 30 ms | shared |
| RPC P95 | < 1 ms | < 1 ms | shared |

`bench --json` returns `platform`, `boot_mode`, `boot_samples_ms`, `boot_sample_count`, `boot_ms_p50`/`boot_ms_p90`, `cold_spawn_ms_p50`, `boot_limit_ms`, `cold_spawn_limit_ms`, and `write_limit_us`; release gates live under `gates`, while `boot_gate_trusted`, `boot_gate_status` and `reliability` live under `advisory_gates` and are not counted in `passed/total`. `metrics.env` carries `mem_load_pct` and a `note` when the host is above 85% memory. Windows is not hard-coded as a failure.

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
- **[v1.2.1-rc.4.4 Changes](docs/v1.2.1-rc.4.4-Changes.md)** — bench stop/join deadlines and temp-dir cleanup, file-task review routed to the judge, XML open-tag recovery, comparison-axis coverage, and TUI overscroll clamping ([中文](docs/v1.2.1-rc.4.4-变更说明.zh-CN.md))
- **[v1.2.1-rc.4.3 Changes](docs/v1.2.1-rc.4.3-Changes.md)** — one-shot IPC probe for cold spawn, code-vs-file output prompt, comparison-axis coverage, write-task review evidence, and the curated model-report archive ([中文](docs/v1.2.1-rc.4.3-变更说明.zh-CN.md))
- **[Model Test Reports](docs/model-test-reports/README.md)** — ten curated Windows real-machine reports with reusable local-model data, plus extracted CSVs ([中文](docs/model-test-reports/README.zh-CN.md))
- **[v1.2.1-rc.4.2 Changes](docs/v1.2.1-rc.4.2-Changes.md)** — XML-style tool-call recovery, bench RPC connect retries on Windows, `advisory_gates` separation, and concise-vs-complete prompt wording ([中文](docs/v1.2.1-rc.4.2-变更说明.zh-CN.md))
- **[v1.2.1-rc.4.1 Changes](docs/v1.2.1-rc.4.1-Changes.md)** — path/file `must_include` exemption, plain-text tool-call recovery and alias normalization, auditable `data/web-artifacts` copies, and `rubric-final` artifacts; documents the 4B long-flow boundary ([中文](docs/v1.2.1-rc.4.1-变更说明.zh-CN.md))
- **[v1.2.1-rc.4 Changes](docs/v1.2.1-rc.4-Changes.md)** — `must_include` match modes, subjective/banned-word validation, unique bench temp dirs with structured errors, reliability-aware boot gate, duplicate tool-call suppression, search-result artifacts, and WebUI/TUI file attachments ([中文](docs/v1.2.1-rc.4-变更说明.zh-CN.md))
- **[v1.2.1-rc.3.6 Changes](docs/v1.2.1-rc.3.6-Changes.md)** — term-only `must_include` with word-term matching, a self-sorting percentile and consistent bench P50/P90, empty/tool-format retries with correction, fixture `must_not_include` item dropping, and English editions of all top-level binaries ([中文](docs/v1.2.1-rc.3.6-变更说明.zh-CN.md))
- **[v1.2.1-rc.3.5 Changes](docs/v1.2.1-rc.3.5-Changes.md)** — placeholder-meta items are dropped instead of copied, subjective criteria are advisory and stay out of the judge, array `must_include`, and a stronger S3 t6 tool-call prompt ([中文](docs/v1.2.1-rc.3.5-变更说明.zh-CN.md))
- **[v1.2.1-rc.3.4 Changes](docs/v1.2.1-rc.3.4-Changes.md)** — guide schema placeholders and validation (no copied `REVIEW-FINAL`), feedback that no longer echoes the literal phrase, and guide/judge artifacts recorded under `data/review-artifacts` ([中文](docs/v1.2.1-rc.3.4-变更说明.zh-CN.md))
- **[v1.2.1-rc.3.3 Changes](docs/v1.2.1-rc.3.3-Changes.md)** — criterion routing (hard/negative/count/regex to hardcheck; only description items to judge), regex criteria, missing-item reinjection, serial majority voting, warm P90/sample count, memory-load reliability note ([中文](docs/v1.2.1-rc.3.3-变更说明.zh-CN.md))
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

`v1.2.1-rc.4.4` fixes the five findings from the rc.4.3 retest plus a TUI overscroll bug. `Server::stop` now retries its named-pipe wake connection for a 3-second budget and bench joins with a 5-second deadline instead of hanging; bench temp dirs are removed through a `Drop` guard and stale `fa-bench-*` dirs older than 24 hours are cleaned at startup. Explicit write-file tasks route all guide criteria to the judge, carry `tool.fs.write path=... status=success` plus the file body as evidence, and no longer fail because the final answer does not repeat guide-invented words; a successful write followed by malformed XML falls back to a deterministic written-path answer. XML function open tags missing `>` are recovered, comparison answers must cover each requested axis, and both TUIs clamp `chat_scroll` at the top so overscroll no longer delays the next Down. N23–N27 still need real-hardware confirmation; N28 is a test-scorer limitation.

`v1.2.1-rc.4.3` fixes the four findings from the rc.4.2 retest. `serve` now uses a one-shot `ipc::try_connect` for its existing-kernel probe, so a clean install root no longer waits the 3-second pipe retry before starting; normal clients keep the bounded retry. Code/text/plan/translation answers stay in the response unless the user explicitly asks to save or write a file. List and comparison answers map their first-level items to the requested axes, covering performance/overhead when no axes are given. For explicit write-file tasks, review-gate receives the successful write path and content as labelled evidence and checks the file rather than demanding a verbatim answer; the prompt also stops repeated post-write reads. N15–N18 were confirmed by the rc.4.2 retest.

`v1.2.1-rc.4.2` fixes the four findings from the rc.4.1 retest. XML-style function/parameter tool calls are recovered by the context parser, and the Ollama provider salvages a parsed call from an accumulated stream instead of failing the whole turn. Bench creates its RPC directory explicitly and retries the Windows named-pipe connect across the pipe-creation window; `boot_gate_trusted` moves to `advisory_gates` so a low-reliability host is not counted as a failed release gate. The system prompt now distinguishes default brevity from list/compare/enumerate tasks, where every required point must be covered. N10, N12 and N13 were confirmed on real hardware; N15–N18 still need the next Windows retest.

`v1.2.1-rc.4` fixes the five findings from the rc.3.6 retest and adds the attachment/search-artifact features. `must_include` now supports `match: any|all|min`; a multi-term item defaults to `min` with `ceil(n/2)` matches, and hardcheck reports how many requirements were covered. Subjective criteria may not use `must_not_include`, and a banned word that appears in the user input rejects the guide. Bench uses a unique temp directory per run and returns `artifact_missing` / `bench_failed` JSON on error; when `reliability` is not `high`, the boot gate is `advisory` with `boot_gate_raw` preserved. Identical tool calls in one model reply are de-duplicated, and every `tool.call` carries `attempt` and `call_index`. Search/news/fetch results are persisted as JSON artifacts; WebUI/TUI attachments are copied to the default workspace when outside the install root.

`v1.2.1-rc.3.6` fixes the four findings from the rc.3.5 retest and adds English editions of every top-level binary. `must_include` is now term-only (2–12 characters, no concept words or sentences such as `和/与/及/是/的`), and hardcheck requires each whitespace/punctuation-separated term rather than one literal substring; a conceptual phrase is rejected at guide time and retried. `percentile` sorts its own samples, bench computes P50/P90 once, and JSON/gates expose `boot_percentiles_consistent` / `metrics_consistent`. Empty responses get two temperature retries and malformed Ollama tool calls get two retries plus a JSON-format correction, each emitted as `model.retry`; a crash still restarts once. A rubric item whose `must_not_include` contains `REVIEW-FINAL`, `REVIEW-START`, `<...>`, `__FILL`, `{{` or `}}` is dropped as a whole with `review.warn`. The five Chinese executables keep their names; `featheragent-en`, `featheragent-tui-en`, `featheragent-webui-en`, `feather-plugin-en` and `feather-storage-en` are packaged beside them in every Release asset.

`v1.2.1-rc.3.5` fixes the M1/M3 findings from the rc.3.4 real-hardware retest. The guide no longer treats a placeholder example as task content: `GUIDE_SYSTEM` uses `__FILL_*__` tokens, a rubric item that only discusses placeholders/examples is dropped with `review.warn` while real items survive, and `must_include` accepts an array such as `["SYN","ACK"]` and requires every phrase (M1). Subjective criteria such as “concise and clear” are marked advisory and excluded from hardcheck and judge, and a rubric with no executable criterion at all is rejected after retries instead of being accepted because it is non-empty (M3). The S3 t6 prompt now explicitly requires one `tool.fs.read` call before answering (M4). rc.3.4 already made criteria task-specific, stopped feedback from echoing the literal phrase, and recorded every guide/judge rollout under `data/review-artifacts/<session>/`.

`v1.2.1-rc.3.2` fixed the three findings from the rc.3.1 real-hardware retest without touching the architecture: `min_points` became a soft item handed to the judge (with a broader Chinese-aware point count and a `review.warn`), review feedback used `〔1〕` labels instead of the bare `c1:` prefixes the exec model copied into answers, and `bench --json` carried a `metrics.env` fingerprint so numbers can be compared across runs.

`v1.2.1-rc.3.1` keeps the rc.3 architecture and fixes what the real-hardware retest found: the guide now gets a literal JSON schema plus a few-shot, its validation error is reinjected into retries instead of resending the same prompt, and a rubric with only descriptions is retried once and then evaluated judge-only with a `review.warn` event rather than degrading the whole round. Plugin settings gained `options_source`, so model fields show a real dropdown fed by the local model service while still accepting a typed name; the TUI plugin page saves on Enter from any field; permission prompts now show the full command/arguments on both front ends; and the product display is **FeatherAgent** (binaries and paths unchanged).

`v1.2.1-rc.3` made plugin settings a declaration: a plugin states its settings schema in `plugins/<name>/plugin.json` (`manifest.settings`: root, title, description, and `bool|int|number|string|enum` fields with range/options), the kernel relays it through the `plugin.settings` RPC and validates writes in `config.set`, and the TUI and WebUI render any plugin's form generically. It also closed the review-gate and S3 findings H1–H4 from the rc.2.1 retest: phrase-only rubrics with four criterion kinds and a pre-judge hardcheck, judge evidence that must be a verbatim ≥10-character window of the answer, an explicit “unknown tool + available tools” correction, and S3 t5/t6 prompts that ask for a single tool call.

Linux automation has **198 tests: 197 passed, 1 ignored by default** (76 lib, 33 plugin ×2, 2 kernel, 6 TUI, 1 TUI-en, 4 WebUI ×2, 2 golden, 37 integration); the English worker/WebUI aliases repeat the plugin and WebUI modules, so those cases run twice. The ignored network-dependent test needs public network. `fmt` and `clippy -D warnings` pass. Release bench: kernel **2,568,520 B (2.45 MiB)**; warm P50 **1.221 / 1.198 / 1.174 ms**; cold spawn P50 **3.567 / 3.060 / 3.099 ms**; all seven release gates are green and `reliability:"high"`. Settings pages were verified at rc.3 on both front ends: TUI through a PTY and the WebUI bridge over WebSocket.

Windows x64 has gone through nine real-hardware rounds from v1.1.3 to v1.1.9, then rc.4.1, rc.4.2 and rc.4.3 retests. The rc.4.3 report confirmed N20 and found N23–N27: a bench hang after step 2, a write task still failed against guide-invented words, XML recovery regressed, TCP/UDP coverage stayed at two axes, and timed-out bench runs left temp dirs. This candidate fixes those five and the TUI overscroll. The v1.2.1-rc.4.4 Windows package contains ten exes (five Chinese, five English `-en`) and is cross-compiled; its automation runs only on Linux. N23–N27 still need real-hardware confirmation. Wine 6.0.3 passed `version` and three `bench --json` runs with a temporary `bcryptprimitives` stub; boot/RSS under Wine are not Windows conclusions.
