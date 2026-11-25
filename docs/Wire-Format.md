# FeatherAgent Frontend Contract · Wire Format v1.0

> Applies to FeatherAgent **1.2.1-rc.4.1** (contract API `1.0`).
> This document describes **the bytes actually on the wire**, not a design draft. Any third-party frontend (Python / Node / Go / C# / WebSocket bridge) should implement this rather than guessing framing.

## 1. Transport endpoints

| Platform | Endpoint | Notes |
|---|---|---|
| Windows | named pipe `\\.\pipe\featheragent-<hash>` | default DACL allows same user; `PIPE_REJECT_REMOTE_CLIENTS` |
| Linux / macOS | Unix domain socket `<root>/runtime/ipc/featheragent.sock` | mode `0600` |

After a successful bind, the server writes the **actual endpoint** to:

```text
<root>/runtime/ipc/endpoint
```

Examples:

- Windows: `\\.\pipe\featheragent-a49fcfef058c3da5`
- Unix: `/home/user/fa/runtime/ipc/featheragent.sock`

Third-party frontends should read this file rather than reimplement the `blake3` pipe-name derivation.

Two startup state files also exist:

```text
<root>/runtime/ipc/starting   # serve began initializing; JSON has pid/version/started_at_unix_ms
<root>/runtime/ipc/ready      # endpoint bound and connectable; JSON has endpoint/pid/version
```

`starting` present without `ready` means the kernel is starting, not that the channel is broken. `chat` waits up to 5 s and prints "kernel not ready (starting, about Ns)"; third-party frontends should do the same instead of reporting `ENOENT`. On startup failure/exit, `starting`, `ready`, and `endpoint` are cleaned up.

## 2. Framing: 4-byte little-endian length + body

**Every frame** (request, response, event notification):

```text
+----------------+--------------------------------+
| u32 LE length  | body (length bytes)            |
+----------------+--------------------------------+
```

- `length` is the **body byte count**, excluding the 4-byte header.
- Maximum **64 MiB** (67,108,864 B). Larger frames close the connection with `frame too large`.
- No newline delimiter, no `Content-Length:` LSP header, no big-endian u32. The four failed probes in reports were framing/encoding mismatches, not a silent server.

### 2.1 Body encoding

Selected by a server startup flag and **fixed for the connection lifetime**:

```bash
featheragent serve --codec msgpack   # default
featheragent serve --codec json      # debug / reference client
```

| codec | body encoding | typical first byte |
|---|---|---|
| `msgpack` (default) | MessagePack **named map** (field names as keys, not arrays) | `0x83` / `0x84` etc. `fixmap` |
| `json` | UTF-8 JSON object | `0x7b` (`{`) |

> **The server's `--codec` decides both request and response encoding.** The client must send and decode with the same codec. The v1.1.1 bug was `--codec json` affecting reads while writes stayed MsgPack; fixed in 1.1.2, and `initialize.result.codec` now reports `json` or `msgpack` truthfully.

`initialize` example (JSON codec):

```json
{"jsonrpc":"2.0","id":1,"result":{
  "version":"1.2.1-rc.4.1","contract":"1.0","transport":"pipe",
  "codec":"json",
  "paths":{"install_root":"...","data":"...","workspace":"...","runtime":"..."}
}}
```

## 3. JSON-RPC 2.0 envelope

Request:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
```

Success:

```json
{"jsonrpc":"2.0","id":1,"result":{...}}
```

Error:

```json
{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"..."}}
```

Event notification (no `id`, server-initiated):

```json
{"jsonrpc":"2.0","method":"event","params":{
  "kind":"model.delta","payload":{"session":"cli","text":"...","first":true},
  "seq":4,"ts":1795331532077826435
}}
```

- `seq` is a per-connection monotonically increasing event number.
- `ts` is a Unix nanosecond timestamp (integer).

## 4. Handshake order (must follow)

1. Connect to the endpoint.
2. Send `initialize`, wait for the response; a 3-second client timeout is recommended.
   - A timeout usually means the kernel is not running, the endpoint file is stale, or the codec does not match so the server cannot parse frames.
3. Send `session.create` (`params.session` optional, defaults to `cli`).
4. Send `event.subscribe` (optional `kinds` array; omit for all).
5. Send `chat.send` (`params.text` required; `params.session` must match step 3).
6. Read frames in a loop and handle `method == "event"` until `params.kind == "chat.finished"`.

### 4.1 Minimal Python client (JSON codec)

```python
import socket, struct, json

def send(sock, obj):
    body = json.dumps(obj, ensure_ascii=False).encode()
    sock.sendall(struct.pack("<I", len(body)) + body)

def recv_exact(sock, n):
    out = b""
    while len(out) < n:
        b = sock.recv(n - len(out))
        if not b:
            raise EOFError
        out += b
    return out

def recv(sock):
    n = struct.unpack("<I", recv_exact(sock, 4))[0]
    return json.loads(recv_exact(sock, n))

s = socket.socket(socket.AF_UNIX)
s.connect("/path/to/runtime/ipc/featheragent.sock")
send(s, {"jsonrpc":"2.0","id":1,"method":"initialize","params":{}})
print(recv(s))
```

On Windows, Python can use `open(r'\\.\pipe\featheragent-...', 'r+b', buffering=0)`; read/write the same "4-byte LE length + body". Do **not** use `readline()`.

## 5. Method set (contract 1.0)

| Method | Purpose | Key params |
|---|---|---|
| `initialize` | handshake, returns version/paths/actual codec | none |
| `system.info` | kernel state, warnings, gates | none |
| `session.create` | create/reuse a session | `session` (optional), `workspace` (project name, optional) |
| `session.list` | list sessions | none |
| `session.resume` | resume an existing session | `session` |
| `session.fork` | fork a session | `session`, `as`, `from_seq`, etc. |
| `event.subscribe` | subscribe to events | `kinds` (optional string array) |
| `chat.send` | send user text, stream events | `text` required, `session` |
| `chat.stop` / `chat.cancel` | emergency stop preserving context / compatible alias | `session` |
| `chat.inject` | insert steering during a run (interrupt then continue) | `session`, `text` |
| `tool.confirm` / `tool.deny` | confirm/deny a pending tool | `token` (from `tool.confirm_request`), `allow` |
| `ctx.inspect` | inspect context assembly | implementation-defined |
| `context.compact` | manually compact old turns into a checkpoint | `session` |
| `workspace.list` / `workspace.create` | list/create project workspaces | `name` (create only) |
| `search.status` | web-search configuration state (does not validate the key) | none |
| `config.get` / `config.set` / `config.replace` | read/modify runtime config | `config.get`: no params for full config; `path` accepts `temperature` or `/temperature`, returns `{value, config}`. `config.set`: `{patch}` or `{path,value}`. `config.replace`: `{config}` full replacement |
| `provider.models` | read provider model list | `provider`, `base_url`, `model` (all optional) |
| `storage.stats` / `storage.gc` | storage stats/GC | none |
| `plugin.list` | plugin inventory | none |
| `plugin.enable` / `plugin.disable` | enable/disable plugin | `name` |
| `plugin.install` | install plugin | `path` (absolute directory containing `plugin.json`) |
| `plugin.settings` | read a plugin's declared settings schema and values | `name` |
| `file.attach` / `file.prepare` | upload or resolve a local attachment; returns the model-facing path | `path`, or `name` + `data_base64` |

Full parameter details live in `src/server/mod.rs::dispatch`; this document locks down the **transport** so third parties do not get stuck on framing. Tool calls have no standalone RPC: the model/session triggers them through `chat.send`, and results return as events. `model.message.finish` may be `tool_calls`, followed by `tool.call` / `tool.result`. Business errors (path escape/missing args/file not found) are `tool.error`; only plugin crash/timeout/IO are `tool.failed`; three consecutive technical failures emit `tool.degraded`. Providers must put structured tool calls into the `done` event, not just concatenated text.

### 5.1 Tool confirmation and denial (v1.1.4)

- When a dangerous tool needs confirmation, the kernel broadcasts a **notification** `tool.confirm_request` with `session` / `token` / `tool` / `risk`, plus (v1.2.1-rc.3.1) `arguments` (the exact JSON the model produced) and `preview` (a human-readable rendering: `$ <command>` then remaining params for `tool.shell`, pretty JSON otherwise, truncated at 4 KB). Frontends should show `preview` and must tolerate its absence (older kernels).
- The frontend replies with `tool.confirm` (`{"token":"...","allow":true}`) or `tool.deny` (`{"token":"..."}`).
- A denial emits `permission.denied` plus a structured `tool.result`: `{"denied":true,"result":{"error":"permission_denied","reason":"user_denied|confirm_timeout","tool":"..."}}`, so the model can explain it next round.
- The `chat` command supports `--allow <tool,...>` / `--yes`; otherwise it denies immediately by default (`--confirm-timeout` keeps a human window).
- An empty-response retry emits `model.retry` (`attempt` / `reason` / `temperature`); still failing → `chat.finished status=failed`.

### 5.2 Settings and model discovery (v1.1.7)

Frontend settings use this path; do not edit `config.json` directly:

```json
// 1) read
{"method":"config.get","params":{}}
// → {"provider_plugin":"provider-ollama","base_url":"...","model":"qwen3.5:4b",
//    "temperature":0.7,"context_window":8192,"max_turns":6,"auto_allow_safe":true,
//    "workspace_root":"",...}

// 2) discover models (Ollama /api/tags; OpenAI-compatible /v1/models)
{"method":"provider.models","params":{"provider":"provider-ollama","base_url":"http://127.0.0.1:11434","model":"qwen3.5:4b"}}
// → {"provider":"provider-ollama","models":["qwen3.5:4b","..."],"probe":{...}}
// models are de-duplicated; failure still returns 200: {"provider":"...","models":[],"error":"..."}; allow manual entry

// 3a) save one field (atomic read-modify-write; recommended for scripts/multi-frontend)
{"method":"config.set","params":{"path":"temperature","value":0.6}}
// → {"config":{...full new config...}}

// 3b) save multiple fields (only changed fields; used by WebUI/TUI)
{"method":"config.set","params":{"patch":{
  "provider_plugin":"provider-ollama","model":"qwen3.5:4b","temperature":0.6,
  "context_window":8192,"max_turns":6,"auto_allow_safe":true
}}}
// → {"config":{...full new config...}}

// 3c) full replacement (migration/import)
{"method":"config.replace","params":{"config":{...full Config...}}}
```

- `config.set` / `config.replace` run under the kernel config lock: read → modify → validate → temp file + atomic replace → update memory → `config.changed`. Concurrent saves from two frontends cannot overwrite each other. `config.set` `path` accepts `temperature` or `/temperature`; unknown fields return `-32602`.
- Only `config.replace` means full replacement; never treat a `config.set` patch as replace.
- Temperature 0.0–2.0; `context_window` 512–131072; `max_turns` 1–32; `provider_plugin` must be registered or the server returns `-32602`.
- Empty-response retry stays at `temperature=0.7`, independent of user settings.
- Empty `workspace_root` means `<install root>/workspace`; relative paths resolve against the install root, absolute paths may point outside. It is a **project container**, not a per-session sandbox: `<workspace_root>/<project>/` is the workspace. It is a **startup-time config**: `config.set` persists and returns immediately, but the running kernel does not hot-migrate; restart `serve`.
- Each project workspace owns `<workspace_root>/<project>/.featheagent/{worklog.md,memory.md,README.md}` (existing content is not overwritten). The kernel appends worklog entries (real UTC time, goal, tools, state); models never write timestamps. No `.featheagent` is created at the container root.
- Sessions may bind a project with `workspace`; old logs without a workspace fall back to `default`. Tool paths `workspace/xxx` and `xxx` both land in the current project root.
- `ctx.inspect` is diagnostic and does not guarantee a session lock: when busy it returns `state:"busy"`, `snapshot:null`; do not treat that as a failed turn.

### 5.3 Plugin management (v1.1.7)

Plugin management is entirely RPC; do not edit `data/plugins/registry.json`:

```json
// list: registry is the registry, live is runtime state
{"method":"plugin.list","params":{}}
// → {"registry":[{"name":"tool","program":"...","args":[],"category":"tool",
//                 "enabled":true,"blake3":"..."}],
//    "live":[{"name":"tool","state":"ready|stopped","manifest":{...}}]}

// install: path is an absolute directory containing plugin.json
{"method":"plugin.install","params":{"path":"D:\\plugins\\my-tool"}}
// → {"installed":{"name":"my-tool","program":"...","enabled":true,...}}

// enable/disable: sync registry and running process
{"method":"plugin.enable","params":{"name":"my-tool"}}
{"method":"plugin.disable","params":{"name":"my-tool"}}

// settings: the plugin declares the schema, the kernel relays it verbatim
{"method":"plugin.settings","params":{"name":"review-gate"}}
// → {"name":"review-gate","root":"review_gate","title":"...","description":"...",
//    "fields":[{"key":"max_rounds","label":"Max rounds","type":"int",
//               "default":2,"min":1,"max":5,"step":1}, ...],
//    "values":{"enabled":false,"max_rounds":3,...},
//    "defaults":{...}}
```

- A successful `plugin.install` with an enabled plugin registers the spec immediately; `plugin.enable` re-registers and starts on demand; `plugin.disable` terminates the running process.
- `plugin.list.registry` can drive dynamic sidebars; `live.state=ready` means running, `stopped` means on-demand.
- Plugin manifest `api` must satisfy `>=1.0 <2.0`; otherwise installation fails.
- `manifest.settings` (optional, v1.2.1-rc.3; extended in v1.2.1-rc.3.1) is the only source of plugin settings UI:
  - `root` — config path holding these fields (default `plugin_settings.<name>`);
  - `title` / `description` — shown above the form;
  - `fields[]` — `key`, optional absolute `path` (to point at an existing top-level key), `label`, `type` (`bool|int|number|string|enum` only), `default`, `description`, `placeholder`, `min`, `max`, `step`, `options`, `options_source` (`""` or `"models"`), `allow_custom`, `secret`.
  - `options_source:"models"` does not mean the kernel resolves the list: the kernel relays the declaration only. Frontends call the existing `provider.models` RPC and render a picker; `allow_custom:true` means a value outside the list must still be accepted (a model name the service does not report).
- A plugin that declares no settings still appears in `plugin.list`; `plugin.settings` returns a business error (“plugin X declares no settings”). Frontends should fall back to the manifest summary plus the enable toggle.
- Writes go through `config.set` with the expanded paths (`<root>.<key>`). The kernel validates type/range/enum and rejects undeclared keys under `plugin_settings.<name>`; first write to a declared path creates missing parents. Frontends must not hard-code plugin field names — render from `plugin.settings`.
- `registry[]` entries carry `settings` (the same schema) so a sidebar can be built before any plugin process starts.

### 5.4 Tool error events

| kind | Semantics | Counts toward breaker? | Frontend advice |
|---|---|---|---|
| `tool.result` | success, or structured permission denial (`denied=true`) | no | show result; red for denial |
| `tool.error` | business error (path escape/missing arg/file missing) | **no** | show "business error"; model receives a tool message |
| `tool.failed` | plugin crash/timeout/IO | yes (by tool name) | show "plugin failure"; after 3, show breaker |
| `tool.degraded` | tool tripped after 3 consecutive technical failures | — | tool removed from later schemas; system hint forbids `tool_call` syntax |

## 6. Error codes

| code | Meaning |
|---|---|
| `-32601` | method not found |
| `-32602` | invalid params |
| `-32603` | internal kernel error |
| `40301` | observer is read-only; write forbidden |
| `40401` | session not found |
| `42901` | budget/quota exceeded |
| `50201` | plugin call failed |
| `40901` | resource busy (e.g., single-client capability held) |

## 7. Multi-client and disconnect semantics

(1.1.2; 1.1.3 cancel; 1.1.4 confirmation; 1.1.5 settings; 1.1.7 plugins/workspace; 1.1.8 stop/steer/compact/search)

- Windows named pipes use **overlapped I/O + one handle per connection**, so reads and writes do not block each other.
- If `accept` sees a client connect and immediately disconnect (`ERROR_NO_DATA`), the instance is reset and an error returned; `Server::run` continues accepting and does not permanently occupy the instance.
- A client read thread treats `ERROR_BROKEN_PIPE` / `ERROR_PIPE_NOT_CONNECTED` as EOF.
- WebUI / TUI / third-party clients can connect concurrently; events are pushed per connection.
- A single-client write operation (some governance actions) may still return `40901`; that is contract semantics, not a pipe deadlock.
- Since 1.1.3, if a session is `Assembling` / `Generating` / `ToolExecuting` when the connection drops, the kernel cancels the request, emits `tool.cancelled` (`reason=client_disconnect`), and releases the session holder; `Cancelled` / `Failed` allow the next `chat.send` to re-enter.

## 8. Compatibility boundary

- `--codec json` is a **debug/reference** capability; WebUI and TUI use MsgPack, so default `serve` is enough.
- A third party choosing JSON must start the server with `--codec json` and verify `initialize.result.codec`; do not send requests if they differ.
- Framing/limit/method set belong to contract `1.0`; new methods are additive, and breaking changes bump `CONTRACT_API`.

### 5.4 Project workspace, stop/steer, compact, and web search (v1.1.8)

```json
// project container: root is the container, workspaces are direct children
{"method":"workspace.list","params":{}}
// → {"root":".../workspace","workspaces":[{"name":"default","path":".../default",
//    "has_agent":true,"has_hidden":false}]}

{"method":"workspace.create","params":{"name":"proj-a"}}
// → {"ok":true,"name":"proj-a","path":".../workspace/proj-a","workspaces":[...]}

// session binds a project
{"method":"session.create","params":{"session":"s1","workspace":"proj-a"}}
// → {"session":"s1","workspace":"proj-a"}

// emergency stop: keeps generated text; status=stopped (chat.cancel returns cancelled)
{"method":"chat.stop","params":{"session":"s1"}}
// → {"ok":true,"preserve_context":true}

// in-turn steer: only while running
{"method":"chat.inject","params":{"session":"s1","text":"don't edit code yet, list a plan first"}}
// → {"ok":true,"during_turn":true}

// manual compact: at least two ChatFinished; keeps the last two complete turns
{"method":"context.compact","params":{"session":"s1"}}
// → {"ok":true,"summary":"...","covers_until_seq":42,"compacted_events":30,
//    "before_chars":9000,"after_chars":600}

// web-search state (configured only means the env var is non-empty)
{"method":"search.status","params":{}}
// → {"provider":"deepseek","base_url":"https://api.deepseek.com",
//    "key_env":"DEEPSEEK_API_KEY","configured":false,"count":5,
//    "billing":"Search/Fetch billed per successful request, 5 CNY / 1000"}
```

### 5.5 File attachments (v1.2.1-rc.4)

`file.attach` and `file.prepare` are dispatched by the same `src/server/mod.rs` handler. WebUI calls them over HTTP RPC (`POST /rpc`, JSON) because the WebSocket frame limit is still 1 MiB; TUI uses the normal MsgPack connection. Either `path` (local, canonicalized) or `name` + `data_base64` (browser upload) is accepted; an empty object is invalid.

```json
{"method":"file.attach","params":{"name":"note.txt","data_base64":"aGVsbG8="}}
// → {"path":".../workspace/default/uploads/...-note.txt","copied":true,
//    "source":"upload","name":"note.txt","bytes":5}
```

A local file inside the install root returns `copied:false` and its canonical path; an outside file is copied to the default workspace. Browser bytes always land in the default workspace. The model receives only `result.path`, never the browser path or the outside source.

- `chat.finished.status` adds `stopped`; old clients should treat both `stopped` and `cancelled` as "stopped by the user". Partial output is preserved as `model.message(finish=interrupted)`.
- 1.2.1-rc.4.1: no new frame, endpoint, event kind, or error code. Tool results for `web.search|news|fetch` add `audit_artifact` beside `artifact` / `artifact_kind` (the audit file lives under `<data>/web-artifacts/<session>/`); `review.guide` adds `final_artifact` pointing at `<data>/review-artifacts/<session>/<ms>-rubric-final.json`. `review.guide` keeps `artifact` for the accepted attempt. Plain-text `tool_call <name> {...}` in a model answer is a compatibility fallback inside the context parser, not a wire change; standard providers should still send structured `tool_calls`. All additive.
- 1.2.1-rc.4: `file.attach` / `file.prepare` are additive methods on the same HTTP RPC `/rpc` endpoint used by WebUI; request `{path}` or `{name,data_base64}`, response `{path,copied,source,name,bytes}`. Errors use `-32602` with `attachment_not_found|attachment_not_file|attachment_empty|attachment_too_large|attachment_write_failed|attachment_copy_failed|attachment_invalid_base64`. No new frame, endpoint, or event kind. Tool results for `web.search|news|fetch` may add `artifact` / `artifact_kind`; bench JSON adds `boot_gate_raw`, `boot_gate_trusted`, `boot_gate_status`, and `reliability`; `tool.call` adds `attempt` and `call_index`. `must_include` adds `match` / `min_matches`. All additive; prompt text and plugin internals only otherwise.
- 1.2.1-rc.3.6: no new wire message, frame, endpoint, or error code. Bench JSON adds `boot_percentiles_consistent` (P50 ≤ P90 from the same sorted samples) and `gates.metrics_consistent`; both are additive. `review.warn` may now report a fixture item dropped because its `must_not_include` contained `REVIEW-FINAL` / `REVIEW-START` / `<...>` / `__FILL` / `{{` / `}}`. `must_include` is interpreted as whitespace/punctuation-separated terms during hardcheck; prompt text changed, not the wire contract. The English `-en` binaries use the same protocol.
- 1.2.1-rc.3.5: no new wire message, frame, endpoint, or error code. `review.parse_rubric` adds `advisory` (ids of subjective items excluded from hardcheck and judge); `review.warn` may report dropped placeholder/example items (`已丢弃占位符/示例判据 …`) and subjective downgrades. `must_include` accepts a string array and hardcheck requires every phrase. A rubric with no executable criterion after retries now fails `review_prepare` instead of proceeding. `GUIDE_SYSTEM` placeholder tokens changed to `__FILL_*__`; this affects prompt text only, not the wire contract.
- 1.2.1-rc.3.4: no new wire message, frame, endpoint, or error code. `review.guide` payload adds `artifact` (path or empty string when writing failed). `review.verdict` adds `attempts` (judge model calls), `votes` (completed votes, 2 when the third is skipped by early stop) and `artifacts` (judge artifact paths). Guide and judge rollouts are written under `<data>/review-artifacts/<session>/` before parsing; this is best-effort and never fails the gate. Rubric parsing now also receives the user input, so `criterion` / `check` / `must_include` / `must_not_include` placeholders and `REVIEW-FINAL` / `REVIEW-START` tokens absent from the task are rejected. No setting field changed.
- 1.2.1-rc.3.3: no new wire message or field. `review.hardcheck` / `review.judge` are plugin-internal: hard/negative/count/glob/regex criteria are settled by hardcheck, only description items reach the judge, and the judge evidence rule is unchanged for those items. `review.warn` text changes for soft `min_points` (“按软判据接受…”) and for unevaluated description items. `plugins/review-gate/plugin.json` declares five additional settings (routing, pattern engine, votes, aggregation, early stop) relayed by `plugin.settings`; `judge.votes` accepts only 1 or 3. Bench JSON adds `boot_ms_p90`, `boot_sample_count`, `env.reliability` and an optional `env.note`. No frame, endpoint, or error code changed.
- 1.2.1-rc.3.2: no new message or field. `review.hardcheck` (plugin-internal, not wire) now returns a `soft` array for `min_points` shortfalls; the engine emits `review.warn` (`text` starts with “min_points 机器计数不足，交 judge 复核”) and appends a machine-count note to the judge input, so the judge decides from the answer instead of a hard rejection. `review.feedback.text` now labels failed items `〔1〕`/`〔2〕` instead of bare `c1:` prefixes; the event payload (`round`/`failed`/`text`/`next_temperature`) is unchanged.
- 1.2.1-rc.3.1: `review.guide` (25), `review.verdict` (26), `review.finished` (27), `review.feedback` (28) are joined by `review.warn` (29, payload `{text, model, attempts, retry?}`). A warn means “evaluated with a weaker rubric” or “this attempt was rejected and retried”, never “the gate failed”; a rubric with description-only items is evaluated judge-only and warned, while a guide that cannot produce any criterion still ends in `degraded`. `review.guide` payload `attempts` tells which attempt produced the rubric; `review.guide` plugin call accepts an optional `feedback` argument carrying the previous validation error.
- 1.2.1-rc.2.1: when review-gate is enabled and the turn completes, `chat.finished.status` is `ok` (approved), `unapproved` (round limit reached without approval), or `degraded` (guide/judge failed twice). The payload `review.status` mirrors `approved|unapproved|degraded`; `review.verdict.items[]` carries `criterion` and `check` in addition to `evidence`. Clients must not assume `ok` means reviewed-and-approved.

- `chat.inject` writes `user.message` (`source:"steer"`, `during_turn:true`) then interrupts generation; an idle session returns a business error, so use `chat.send`.
- Compaction writes `context.compacted` (code 24) with `summary` / `covers_until_seq` / `compacted_events` / `before_chars` / `after_chars`; the model view keeps only events after the cutoff, while raw text remains in the append-only log.
- `web.search` / `web.fetch` / `web.news` / `web.suggest` / `web.status` are absent when `search_provider=none`; `local` needs no key, `deepseek` does. Web results are always `untrusted:true`. Local search is keyless; DeepSeek billing is in `docs/v1.2.0-rc.1-Changes.md` §2.
- `web.search` / `web.news` `count` is declared `integer`; when a small model sends a numeric string (e.g. `"5"`), the kernel normalizes it to an integer before forwarding and preserves the original `tool.call` params. Clients must not depend on the string type.
