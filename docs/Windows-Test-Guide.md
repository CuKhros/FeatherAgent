# FeatherAgent 1.2.1-rc.2 · Windows x64 Test Guide

> Package under test: `FeatherAgent-1.2.1-rc.2-windows-x64.zip`
> Baseline: v1.1.9 real-hardware retest report (Q1/Q2)
> Note: v1.2.1-rc.2 is the rc.1 retest patch: it fixes the empty provider request body when the API key is blank, enforces review-gate temperature ranges even while disabled, prints the effective model/provider/base URL at startup, and makes embeddings use the active model. R1 and review-gate behavior are unchanged. **Windows conclusions must come from your real-hardware retest**; Wine only verifies basic startup and does not prove named-pipe concurrency, ConPTY, proxy behavior, or real kill timing.

## 1. Unzip and start

Unzip into a writable directory (not `Program Files`):

```powershell
D:\FeatherAgent-1.2.1-rc.2\
```

First launch may take 1–2 minutes: Windows antivirus scans each plugin binary on first execution, and the kernel prints:

```text
[init] registering built-in plugin storage (first run may be scanned by antivirus)
[init] registering built-in plugin tool (first run may be scanned by antivirus)
...
```

Wait for `serve ready: 1.2.1-rc.2`, then continue.

```powershell
cd D:\FeatherAgent-1.2.1-rc.2
.\featheragent.exe help
.\featheragent.exe serve --root . --provider provider-ollama --model qwen3.5:4b
```

In another terminal:

```powershell
.\featheragent-tui.exe --root .
.\featheragent-webui.exe --root . --app
.\featheragent.exe chat --root . --text "hello"
```

Both frontends are Chinese-only; there is no `--lang` switch.

## 2. S1 project-level workspace

`workspace_root` is a **project container**; project directories inside it are the actual workspaces:

```powershell
# External container (or set workspace_root in WebUI/TUI settings and restart serve)
.\featheragent.exe serve --root . --workspace D:\Projects\fa-ws --provider provider-ollama --model qwen3.5:4b
```

Expected:

```text
D:\Projects\fa-ws\
├── default\
│   ├── .featheagent\worklog.md
│   ├── .featheagent\memory.md
│   └── .featheagent\README.md
└── proj-a\                  ← after creating a project/session
    └── .featheagent\
```

Each project must have its own `.featheagent`, not one shared copy at the container root. Ask the model to write `proj-a/hello.txt`; it should land in `D:\Projects\fa-ws\proj-a\` and not in `default`.

TUI:

```text
/workspace proj-a       # create a project
/workspaces             # list projects and .featheagent state
/new s1 proj-a          # bind the session to proj-a
```

WebUI: use the project dropdown + "＋ Project", choose a project when creating a session; resumed sessions show the project name.

## 3. S2 stop, steer, compact

### Emergency stop with context preservation

Send a long task (or `SPIN`), then during generation:

- TUI `/stop`, or WebUI "Stop & Save".
- Expected: `chat.finished(status=stopped)`; generated partial text remains in the session; `worklog.md` remains; the next message can be sent immediately.

### In-turn steering

During generation, type new text (TUI: press Enter; WebUI: input remains available):

- Expected: `user.message(source=steer, during_turn=true)`; current generation is interrupted, and the model sees the steer in the next round.
- If no turn is running, `chat.inject` advises using `chat.send`.

### Compact context

After at least two turns:

- TUI `/compact`; WebUI "Compact context".
- Expected: a `context.compacted` event; the summary covers old events; the model view no longer contains old raw text, but the raw text remains replayable; the session resumes after restart.

## 4. S3 local web search (default, no key)

The Settings "Search provider" defaults to `local` and needs no API key. Expected:

- Ask "search today's Liuzhou weather"; see `tool.call` → `web.search` → `tool.result` with `title/url/snippet/source/rank` and `untrusted:true`.
- Chinese network uses Bing; technical questions add Hacker News; `engine=all` adds GitHub/Wikipedia.
- `web.news` reads Google News RSS and falls back to China News RSS (`source=chinanews`, `query_matched=false` means latest headlines only).
- `web.fetch` fetches public pages; `localhost` and private networks are rejected.
- `web.suggest` returns suggestions; `web.status` returns `provider=local`, engine list, `api_key_required=false`.

**DeepSeek (optional)**: switch to `deepseek` and provide `DEEPSEEK_API_KEY`; billed per successful request (5 CNY / 1,000 requests). Set `search_provider` to `none` to disable network search.

## 5. Markdown and icons

- TUI: send `# heading`, `- list`, `1. ordered`, `> quote`, code block, `**bold**`, `*italic*`, `[link](https://…)`; the assistant reply should drop Markdown markers while keeping structure.
- WebUI: the same message renders headings, lists, quotes, code blocks, and clickable links; assistant messages no longer show raw `**` or triple backticks.
- Taskbar/program icons: all five exes show the cyan feather icon; WebUI top-left and favicon show it too.

## 6. Regression commands

```powershell
# NEW-K: bench while a kernel runs must exit 2
.\featheragent.exe bench --root . --json
echo "bench exit=$LASTEXITCODE"

# Gates (kernel stopped)
.\featheragent.exe bench --root . --json

# Tool loop + disk write
.\featheragent.exe chat --root . --session s3 --text "write a file" --allow tool.fs.write
```

Expected: bench conflict prints "kernel already running (pid N)" and `exit=2`, not a swallowed 0.

R1 checks (kernel stopped, release build):

- `metrics.boot_mode` is `warm_in_process_same_install_root`; `boot_samples_ms` has 5 numbers; `boot_warmup_discarded_ms` is present.
- `gates.boot_lt_100ms` is true for **warm** startup.
- `metrics.cold_spawn_ms_p50` is present and `gates.cold_spawn_lt_250ms` is true. Cold spawn is real `serve` process startup, not the in-process number.
- Run bench 3 times; the warm result should stay under 100 ms without the 130–175 ms outliers from the old gate.

Q1/Q2 checks:

- After `/stop`, the TUI status bar shows `cancelled` (source and binary verified; `findstr /C:"cancelled" .\featheragent-tui.exe`).
- When the model sends `count` as `"5"`, `web.search` still returns 5 results; `tool.result.count` matches the request.
- `web.search` schema remains `integer`; string tolerance happens only in the kernel forwarding layer and does not change the external contract.

v1.2.1-rc.2 regression checks:

1. Start real Ollama with `provider-ollama`, empty API key; send a normal message. It must not return `400 missing request body`. Inspect the server/proxy log: `POST /api/chat` has a non-zero `Content-Length` and a JSON body with `"model"` and `"messages"`; `/api/embed` also has a body.
2. While review-gate is **disabled**, call `config.set` with `review_gate.judge.temperature=0.8`. It must be rejected with `超出允许区间 [0.0, 0.2]`, and `config.json` must keep the old value. Repeat guide 0.9 and exec 1.9.
3. Start `serve` with a `--model` different from `config.json`. The banner must print `effective model=<cli value> ... (model_source=cli ...)` and a `warn: --model 覆盖 config.json ...` line. `config.get` and the `review.guide` event payload must return the CLI model.
4. Enable review-gate and repeat the `REVIEW_START` flow; the guide/verdict/feedback/finished sequence must be unchanged.

v1.2.1 additional checks:

1. Settings shows `provider-llamacpp`, `provider-ninfer`, `provider-lmstudio`, `provider-vllm`, and `provider-openai`; changing provider fills the corresponding default base URL unless a custom URL is already present.
2. Start a local OpenAI-compatible server (llama.cpp `--server` or NInfer). Pick the matching provider, click the model-list refresh, chat, and confirm the reply comes from the local server.
3. Fill the optional `API Key` field; confirm the local server/proxy receives `Authorization: Bearer <key>`. Clear it and confirm the header is absent.
4. Enable `三段校验 review-gate` with `provider-mock`; send `REVIEW_START`. Expect `review.guide`, at least one failing `review.verdict`, `review.feedback`, a second `review.verdict` that passes, and final answer `REVIEW-FINAL` with `chat.finished.review.approved=true`.
5. Set guide/exec/judge temperatures outside 0.2–0.5 / 0.6–1.5 / 0.0–0.2; saving must be rejected.

## 7. Known boundaries

- N2 adds `[init]` progress but **does not remove** the first-run antivirus scan time.
- Old `<workspace_root>/<session>/` empty directories are not auto-cleaned; delete manually after confirming they are empty.
- WebUI Markdown/icons were verified in a real Linux browser; TUI Markdown is unit-tested only and still needs PTY confirmation.
- v1.2.1-rc.2 did not run Wine; passing Wine does not equal passing Windows.
- Still untested: high-DPI/Chinese rendering, real Ollama 7B/4B metrics, proxy environments, network/sync drives, 24h soak.

## 8. Artifact verification

```powershell
Get-FileHash .\FeatherAgent-1.2.1-rc.2-windows-x64.zip -Algorithm SHA256
```

Use the release assets and manifest for the authoritative byte count/hash. All five exes depend only on Windows system DLLs; the root contains `LICENSE`, `SECURITY.md`, `CONTRIBUTING.md`, and `CHANGELOG.md`.
