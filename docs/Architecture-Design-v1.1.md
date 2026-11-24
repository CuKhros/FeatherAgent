# FeatherAgent — Overall Design v1.1

> Version: **v1.1.1** (kernel minimalism + independently distributed frontends; includes review erratum R1) · Date: 2026-11-21 · Status: under review
>
> History: v0.1 → v0.3 (review revisions) → v1.0 (baseline) → **v1.1**
>
> This English edition mirrors the structure of the full Chinese design and summarizes its decisions. The Chinese edition ([FeatherAgent架构设计文档-v1.1.zh-CN.md](FeatherAgent架构设计文档-v1.1.zh-CN.md)) remains the detailed reference for tables and rationale.

## Revision notes (v1.0 → v1.1)

| ID | Change | Impact |
|---|---|---|
| **P1** | **Kernel language Go → Rust** (decision flipped; see §2.3) | all kernel implementation |
| **P2** | **SQLite removed from the kernel**: event log becomes a custom append-only format (§3.1); FTS/vector/knowledge base move into a **separate storage plugin process** (§6) | about -10 MB size, -40 ms startup, event-write P95 from 5 ms to <100 µs |
| **P3** | **goja (JS plugins) removed**; untrusted in-process plugins use **WASM (wasmi)**; JS ecosystem is reached through WASM or out-of-process plugins | about -1.5 MB, better isolation |
| **P4** | **GUI ships in the first release but is independently distributed**; Frontend Contract is defined separately; default form is a browser `--app` host, native shell optional (§7, §8) | distribution splits into three artifacts |
| **P5** | **Roadmap de-timed**: phases + gates instead of dates | §11 |
| **P6** | **Detailed technology inventory added** (explicit user request) | new chapter |
| Other | All v1.0 N1–N8 corrections retained (three size metrics, volume-type guard, three-tier vector hot/cold, retention, IDL contract, supply chain, multi-version adaptation, three-path separation) | — |

### Erratum R1 (second review, 5 issues + 2 wording fixes)

| ID | Problem | Resolution | Section |
|---|---|---|---|
| **R1-1** | Log timestamps used `CLOCK_MONOTONIC` — a **format-level error**: not valid across restarts/processes and meaningless after archiving | Store **wall-clock `CLOCK_REALTIME`**; ordering authority becomes `seq`; monotonic clock only for in-process timing | §3.1 |
| **R1-2** | "GUI uses WS" contradicted the minimalist `mio` kernel (WS would pull tokio+tungstenite and break both the 8 MB and `mio` decisions) | The **kernel implements no HTTP/WS**; the WS bridge belongs to **the GUI package's `featheragent-gui`**; add `127.0.0.1` + one-time token + Origin/Host checks (DNS-rebinding defense) | §7.2 |
| **R1-3** | `chrome --app` used the **user profile** by default, conflicting with zero-user-directory-write and CI auditing | Force `--user-data-dir=<root>/runtime/gui-profile`; frontend is **stateless**; browser is on the external-program allowlist and profile usage is asserted | §7.3 |
| **R1-4** | Go-era leftovers: `wazero` (Go WASM runtime, unusable from Rust); `crc32fast` (IEEE CRC-32) mixed with `crc32c` (Castagnoli) | Standardize on **`wasmi`** + **`crc32c`** | §10.1, §11 |
| **R1-5** | `sonic-rs` SIMD dependency conflicted with the "four platforms × two architectures" promise; no fallback documented | Add `json-fallback` feature to `serde_json`; put the four-platform × two-architecture matrix into the **S1 gate** | §10.1, §11 |
| R1-6 | "SQLite single-event P95 2–5 ms" was overstated (WAL + `synchronous=NORMAL` is about 0.1–1 ms) | P2's justification is **size and write-path shape**, not a latency number | §3.1 |
| R1-7 | `wasmi ~400 KB` was optimistic | "Pending S1 measurement, estimated 0.6–1 MB" | §10.1 |

## 1. Positioning and boundaries

### 1.1 One sentence

> **FeatherAgent = a context engine for 1B–14B local models + a tiny, zero-runtime-dependency, portable agent kernel.**

Three selling points:

1. **Context engineering for small models** (technical differentiation and research focus).
2. **Extremely light kernel** (one Rust binary, target **≤ 8 MB**, idle RSS **≤ 15 MB**).
3. **Portability** (zero runtime dependencies; install root / data / workspace can be separated).

"Everything is a plugin" remains an implementation strategy, not a headline claim.

### 1.2 Relationship to DeepSeek Harness: interoperate, do not replace

dsh (MIT, Cordis kernel, `@deepseek-ai/dsh`) already covers plugins, traceability, configuration composition, and KV cache. FeatherAgent exists for three differences:

| Dimension | dsh | FeatherAgent |
|---|---|---|
| Runtime | requires Node.js | **zero-dependency single binary** (no Node / Python / JVM) |
| Kernel size/startup | Node baseline ~50 MB / hundreds of ms | **≤ 8 MB / < 20 ms warm start** |
| Optimization focus | general harness | **context engineering specialized for 1B–14B** |

Three interoperability paths:

1. `bridge.mcp-client`: consume MCP servers from the dsh ecosystem.
2. `bridge.mcp-server`: expose FeatherAgent tools back to dsh.
3. Preset semantic alignment for easier migration.

> **Kill criterion**: if dsh ships a Node-free single-binary distribution plus a small-model context layer, re-evaluate this project's positioning rather than defending it.

### 1.3 Target users and non-goals

- **Users**: developers and semi-technical users with a local GPU, intranet machine, or NAS; from v1.x the GUI broadens the audience.
- **Non-goals**: no built-in inference engine, no model weights, no cloud account, no telemetry, no mobile client.

### 1.4 Size model (three metrics, never mixed)

| Metric | Definition | v1.1 target | Nature |
|---|---|---|---|
| **Out-of-box gate** | first unzip, zero use, zero user plugins | soft **≤ 100 MiB** / hard **≤ 1 GiB** | **hard CI assertion** |
| **Steady-state usage** | total directory size after typical use | 1–2.5 GiB | monitored |
| **User quota** | configurable soft cap | 4 GiB default | visible in UI + one-click cleanup |

> 1 GiB is an out-of-box hard gate, not a lifetime cap. Growth is managed through quota + visibility + cleanup (§6.5).

## 2. Overall architecture

### 2.1 Layered view (three processes)

```text
Frontend layer (independently distributed and replaceable)
  TUI plugin (core package)   GUI plugin (standalone)   third-party clients (contract)
                         │ ① Frontend Contract (IDL v1, UDS/named pipe + WS)
FeatherAgent kernel (Rust, single binary, target ≤ 8 MB)
  A. Orchestration: event bus (single writer, many readers) · session state machine · loop scheduler
  B. Event log: custom append-only format (no SQLite, §3.1)
  C. Plugin runtime: out-of-process RPC + WASM (wasmi) sandbox
  D. Governance: cooperative permission gate · environment sandbox · volume guard · path resolution
  E. Contract layer: Plugin Contract IDL + Frontend Contract IDL + multi-version adaptation
                         │ ② Plugin Contract (IDL v1, stdio JSON-RPC / MCP-compatible)
Plugin process layer
  context-engine   storage/index (SQLite·FTS·vector)
  provider-*       tool-*       bridge-*
                         │ OpenAI-compatible / native runtime protocol
Local inference runtime (external): Ollama / llama.cpp / LM Studio / vLLM
```

**Two contracts, one kernel**: the kernel understands contracts, not capabilities.

### 2.2 Four kernel invariants (the fourth added in v1.1)

1. **The kernel does not understand agents**: removing any capability (including the loop) must not change the kernel.
2. **Every capability is a plugin**: plugins use a language-neutral, versioned contract.
3. **Everything is traceable**: an append-only event stream supports replay / fork / audit.
4. **The kernel owns no business storage**: it writes only its own append log; all queryable indexes (FTS/vector/knowledge base) live in plugin processes.

> The fourth invariant is the architectural form of P2: SQLite, vectors, and full-text search are removed from the kernel, buying size and startup wins while letting the index layer switch implementations (SQLite / redb / tantivy / external service).

### 2.3 Kernel language: Go → Rust

The v1.0 matrix weighted "1–2 people, six months to v1.0" heavily, which favored Go. After the premise changed (no schedule), that weight became zero. Rust wins on size, startup, memory, and latency determinism; `ratatui` covers the TUI, and the GUI is web-based and language-independent. SQLite/HTTP/JSON no longer constrain the choice because SQLite left the kernel and `sonic-rs`/`hyper` are mature. Go remains a **fallback** because plugins and frontends are isolated by contracts; only the kernel body would be rewritten.

Size techniques: `opt-level="z"`, fat LTO, one codegen unit, `panic=abort`, symbol stripping; no SQLite/goja; `regex-lite`; handwritten error enums; `hashbrown` + `ahash`; `wasmi` instead of `wasmtime`; musl on Linux and static CRT on Windows; `cargo bloat` / `cargo-udeps` in CI.

> The goal is not "smallest possible" but "**under 8 MB without sacrificing maintainability**". If necessary, accept 10 MB rather than `no_std` self-flagellation.

### 2.4 Performance and size targets

| Metric | v1.1 target | Means |
|---|---|---|
| Kernel binary | **≤ 8 MB** | §2.3 |
| Warm start to usable | **< 20 ms** | no SQLite init, no GC warm-up, lazy mmap; real cold spawn is measured separately |
| Idle RSS | **≤ 15 MB** | no DB buffer pool, arena allocation |
| Event write (without fsync) | **P95 < 100 µs** | append + group commit (§3.1) |
| Event dispatch (one hop) | **P50 < 50 µs / P95 < 200 µs** | single-writer multi-reader lock-free queue (§3.2) |
| Context assembly (without summary model) | **P95 < 30 ms** | zero-copy segment stitching, SIMD budget |
| Kernel ↔ plugin RPC round trip | **P95 < 1 ms** | length-prefixed msgpack frames |
| End-to-end first token (8B + prefix cache) | ≤ 1.0 s | stable prefix + batching |

> All metrics enter the CI baseline; a regression over 10% fails the gate.

### 2.5 Decision reversibility

Plugins use out-of-process contracts and frontends use the Frontend Contract, so changing the kernel language does not disturb either. The IDL is language-independent, and the `libfeather` C ABI embedding SDK lets third parties use the context engine directly. A Go fallback would rewrite only the kernel body; plugins and frontends stay unchanged.

## 3. Kernel design

### 3.1 Event log: custom append-only format (replaces SQLite)

The kernel writes segmented append-only logs with CRC32C, `seq` ordering, and wall-clock timestamps. SQLite is not embedded: it costs size, startup time, and a write-path shape that does not fit a single-writer event stream. Segments, headers, records, group commit, recovery, and compaction are specified in the Chinese edition. The key guarantees are:

- append-only records, never in-place mutation;
- `seq` is the ordering authority across restarts;
- CRC32C detects torn writes;
- group commit amortizes `fsync`;
- replay reconstructs state; fork copies a prefix.

### 3.2 Event bus: single writer, many readers

One writer appends; projections and subscribers read. Slow subscribers lag rather than block the writer. Lag is observable and old events may be dropped with a `subscriber.lagged` event.

### 3.3 Plugin runtime

Plugins are separate processes speaking stdio JSON-RPC. The runtime handles spawn, initialize, capability negotiation, request/response correlation, cancellation, crash detection, restart policy, and health state. Untrusted in-process plugins use WASM (wasmi). A crash is isolated from a cancel: cancellation is a protocol outcome; a crash is a failed process.

### 3.4 Path resolution and environment sandbox

Install root, data, workspace, and runtime are resolved independently. The environment sandbox restricts plugin access to declared roots; the governance layer decides per tool call. Path traversal, symlink escape, and volume changes are checked before execution.

### 3.5 Volume-type guard

Operations that cross filesystem boundaries (rename, hard link, atomic replace, lock files) are guarded by volume identity. Windows drive letters, UNC paths, and Linux mount points are normalized before comparison.

## 4. Plugin system

### 4.1 Contract and transport separation

The Plugin Contract IDL defines methods, capabilities, errors, and version negotiation. Transport is stdio JSON-RPC by default; MCP compatibility is a mapping layer, not a second contract.

### 4.2 Multi-version adaptation

The runtime negotiates a contract version at initialize. Older plugins can be adapted; unsupported versions fail closed with a structured error.

### 4.3 Security boundary (honest statement)

Out-of-process plugins are a real security boundary. In-process WASM is a capability boundary but not a perfect sandbox. The governance layer is cooperative: it prevents accidental damage, not a determined local attacker with the same OS account. This is stated explicitly rather than marketed as a sandbox.

### 4.4 Supply chain

Plugin manifests declare program, hash, capabilities, permissions, and version. Installation verifies the hash and records provenance. Unsigned plugins are allowed only with an explicit user action.

### 4.5 Crash vs cancellation

Cancellation is a normal protocol state (`cancelled` / `interrupted`). Crash is a process failure with a distinct event, quarantine, and restart policy. UI state must not conflate them.

## 5. Context engine

The context engine builds a bounded prompt from system instructions, tool schemas, skills, history, and recall. Four compression layers: L1 window messages, L2 summary cards, L3 task state, L4 recalled items. A budgeter allocates tokens by layer; overflow degrades gracefully. Parsing tolerates malformed model output. Prefixes are kept stable for KV-cache reuse.

## 6. Storage and indexing (moved out of the kernel)

### 6.1 Three-layer responsibility split

The kernel owns only the event log. A storage plugin owns indexes, full-text search, vectors, and knowledge-base metadata. Frontends own presentation. This keeps the kernel small and the storage implementation replaceable.

### 6.2 Default storage plugin

SQLite + FTS5 by default; redb, tantivy, or an external service can replace it behind the same contract. The plugin exposes index/query/delete/compact methods and reports its own health.

### 6.3 Vector retrieval: three-tier hot/cold + memory budget

Recent vectors stay in memory; older vectors spill to disk; cold data is rebuilt on demand. The memory budget is explicit and observable.

### 6.4 Replay / fork / re-execute

Replay reconstructs a session from the event log. Fork copies a prefix into a new session. Re-execute replays tool calls with recorded parameters, subject to governance and idempotency.

### 6.5 Data lifecycle and retention

Retention is configurable per data class. Cleanup is visible and one-click. The out-of-box gate is separate from the user quota.

## 7. Frontend system (contract-driven, independently distributed)

### 7.1 Three-path separation

Install root, data, and workspace are separate. Frontends receive paths through the contract; they do not guess.

### 7.2 Frontend Contract

IDL v1 over UDS/named pipes. Framing: 4-byte little-endian length + msgpack/JSON body. Handshake: initialize → initialized → session.create/resume → event subscription. One-time token for WebUI, Host/Origin checks, loopback only. The kernel implements no HTTP/WS; the WebUI bridge owns that.

### 7.3 Three frontend forms

- TUI: ships with the core package, `ratatui`, three panes, keyboard-first.
- WebUI: standalone package, embedded assets, browser `--app` host.
- Third-party: any client speaking the Frontend Contract.

### 7.3.1 Web search and presentation layer (v1.1.9)

Keyless local search plugin with Bing/DuckDuckGo/Wikipedia/HN/GitHub and news/fetch/suggest/status. `web.search` and `web.news` are exposed as tools. Results are rendered in TUI/WebUI Markdown.

### 7.3.2 Version and build alignment (v1.2.0-rc.1)

`Cargo.toml` is the single version source. `scripts/version-check.sh` checks the lock file, five plugin manifests, and the WebUI placeholder. Linux/Windows builds use `--locked`; Windows PE resources carry `FileVersion`/`ProductVersion`; WebUI injects `{{VERSION}}` at startup. `scripts/build-linux.sh`, `scripts/build-windows.sh`, `scripts/package.sh`, and `scripts/package-windows.sh` are the supported entry points. `scripts/ci.sh` runs the local gate.

### 7.4 WebUI features (v1.1.8)

Session list, project workspaces, chat, context panel, settings, plugin management, Markdown rendering, icons, stop/steer/compact, and idle WebSocket recovery.

## 8. Distribution

Three artifacts: `featheragent-core` (kernel + TUI + plugins), `featheragent-webui` (assets + bridge), and `featheragent-plugins` (factory plugins). Windows ships five exes in one zip. Linux ships three tar.gz files. Both include README/LICENSE; Windows also includes security/contributing/conduct/changelog.

## 9. Evaluation and regression

### 9.1 Golden trace regression

Recorded event traces are replayed and compared. Divergence fails CI.

### 9.2 Small-model metric card

Fixed model and fixed prompts make runs comparable: tool success, context retention, compression loss, and latency.

### 9.3 Kernel baseline

Boot, event write, context assembly, RPC, binary size, and idle RSS are measured in release builds. Platform-specific thresholds are explicit; Windows is not silently failed.

## 10. Technology inventory

### 10.1 Kernel (Rust)

`mio` for I/O, `serde`/`serde_json` (optional `sonic-rs`), `crc32c`, `blake3`, `wasmi`, `rusqlite` only in the storage plugin, `ratatui`/`crossterm` in the TUI. No async runtime in the kernel by default; no SQLite; no JS engine.

### 10.2 Frontends

TUI: `ratatui` + `crossterm`. WebUI: embedded HTML/CSS/JS, WebSocket bridge, no framework build step. Optional native shell: `wry`, not required.

### 10.3 Storage / index plugins

SQLite + FTS5 default; redb/tantivy/external service alternatives. Vector storage uses a tiered hot/cold design with an explicit memory budget.

### 10.4 Model access and constrained decoding

OpenAI-compatible HTTP and native runtime protocols. Tool schemas are JSON Schema; decoding constraints are provider-specific and optional.

### 10.5 Engineering and quality

`cargo fmt`, `clippy -D warnings`, unit/integration/golden tests, local CI `scripts/ci.sh`, cross-compilation, PE resource checks, and package manifests.

## 11. Roadmap (phases + gates, no dates)

- **Phase 0** — decision validation: three spikes (Rust size/startup, plugin RPC, WebUI bridge).
- **Phase 1** — kernel core: log, bus, contracts, governance, CLI.
- **Phase 2** — context engine: four layers, budget, parsing, KV-friendly prefixes.
- **Phase 3** — storage, tools, security: storage plugin, tool plugin, permissions, volume guard.
- **Phase 4** — GUI: standalone WebUI, browser host, settings/plugins.
- **Phase 5** — distribution and ecosystem: three artifacts, packages, docs, bridges.
- Later: native GUI plugin, additional providers, optional WASM plugins.

## 12. Risks and mitigations

| Risk | Mitigation |
|---|---|
| Rust size target missed | measure with `cargo bloat`; accept 10 MB rather than break maintainability |
| Small-model tool calling is weak | tolerant parsing, constrained schemas, retries, mock coverage |
| Web search sites change | multiple providers, graceful degradation, no SLA claim |
| Plugin sandbox overstated | explicit boundary statement; out-of-process by default |
| Windows differs from Linux | platform-specific gates, cross-compile checks, real-hardware retest |
| Scope creep | phases + gates; non-goals enforced |

## 13. Full request data flow

1. Frontend sends `chat.send` with session id and text.
2. Kernel appends `chat.user` to the event log.
3. Engine assembles context: system, tools, skills, history, recall.
4. Provider plugin streams tokens; kernel emits `chat.delta`.
5. Tool call → governance decision → tool plugin → result event.
6. Loop continues until finish; kernel appends `chat.finished`.
7. Frontend renders events; TUI/WebUI update panels.
8. Worklog is appended at turn end with UTC time, goal, and tools.

## Appendix A: v1.1 vs v1.0

| Area | v1.0 | v1.1 |
|---|---|---|
| Kernel language | Go | Rust |
| Event log | SQLite | custom append-only |
| Index/vector | in kernel | storage plugin |
| JS plugins | goja | removed; WASM |
| GUI | later | first release, independent |
| Roadmap | dated | phases + gates |
| Size | ~30 MB | ≤ 8 MB target |

## Appendix B: references

- Rust `mio`, `ratatui`, `wasmi`, `crc32c`, `blake3`
- SQLite FTS5, redb, tantivy
- MCP / JSON-RPC 2.0
- Keep a Changelog, Contributor Covenant 2.1
- DeepSeek Harness (dsh) for interoperability comparison
