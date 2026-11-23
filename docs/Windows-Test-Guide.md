# FeatherAgent 1.2.0-rc.1 · Windows x64 Test Guide

> Package under test: `FeatherAgent-1.2.0-rc.1-windows-x64.zip`
> Baseline: v1.1.9 real-hardware retest report (Q1/Q2)
> Note: v1.2.0-rc.1 fixes TUI cancellation spelling and `web.search` numeric-string tolerance, aligns Linux/Windows version and build gates, and adds MIT licensing and public-repository materials. **Windows conclusions must come from your real-hardware retest**; Wine only verifies basic startup and does not prove named-pipe concurrency, ConPTY, proxy behavior, or real kill timing.

## 1. Unzip and start

Unzip into a writable directory (not `Program Files`):

```powershell
D:\FeatherAgent-1.2.0-rc.1\
```

First launch may take 1–2 minutes: Windows antivirus scans each plugin binary on first execution, and the kernel prints:

```text
[init] registering built-in plugin storage (first run may be scanned by antivirus)
[init] registering built-in plugin tool (first run may be scanned by antivirus)
...
```

Wait for `serve ready: 1.2.0-rc.1`, then continue.

```powershell
cd D:\FeatherAgent-1.2.0-rc.1
.\featheragent.exe help
.\featheragent.exe serve --root . --provider provider-ollama --model qwen3.5:4b
```

In another terminal:

```powershell
.\featheragent-tui.exe --root .
.\featheragent-webui.exe --root . --app
.\featheragent.exe chat --root . --text "hello"
```

Both frontends default to English. Add `--lang zh-CN` for the Chinese interface:

```powershell
.\featheragent-tui.exe --root . --lang zh-CN
.\featheragent-webui.exe --root . --app --lang zh-CN
```

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

Q1/Q2 checks:

- After `/stop`, the TUI status bar shows `cancelled` (source and binary verified; `findstr /C:"cancelled" .\featheragent-tui.exe`).
- When the model sends `count` as `"5"`, `web.search` still returns 5 results; `tool.result.count` matches the request.
- `web.search` schema remains `integer`; string tolerance happens only in the kernel forwarding layer and does not change the external contract.

## 7. Known boundaries

- N2 adds `[init]` progress but **does not remove** the first-run antivirus scan time.
- Old `<workspace_root>/<session>/` empty directories are not auto-cleaned; delete manually after confirming they are empty.
- WebUI Markdown/icons were verified in a real Linux browser; TUI Markdown is unit-tested only and still needs PTY confirmation.
- v1.2.0-rc.1 did not run Wine; passing Wine does not equal passing Windows.
- Still untested: high-DPI/Chinese rendering, real Ollama 7B/4B metrics, proxy environments, network/sync drives, 24h soak.

## 8. Artifact verification

```powershell
Get-FileHash .\FeatherAgent-1.2.0-rc.1-windows-x64.zip -Algorithm SHA256
```

Use the release assets and manifest for the authoritative byte count/hash. All five exes depend only on Windows system DLLs; the root contains `LICENSE`, `SECURITY.md`, `CONTRIBUTING.md`, and `CHANGELOG.md`.
