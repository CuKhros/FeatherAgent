# FeatherAgent 1.2.1-rc.3.3 · Windows x64 Test Guide

> Package under test: `FeatherAgent-1.2.1-rc.3.3-windows-x64.zip`
> Baseline: v1.1.9 real-hardware retest report (Q1/Q2)
> Note: v1.2.1-rc.3.3 fixes the rc.3.2 retest findings K1–K4. K1: literal/negative/count/glob/regex criteria are settled by `review.hardcheck`; only description items reach the judge, and evidence-verbatim applies only there. K2: a judge that omits an item gets the full id list reinjected and retries inside the same vote instead of degrading the round. K3: `bench --json` adds warm P90 and sample count. K4: `metrics.env` adds a memory-load `reliability` note that annotates a busy host without turning a gate red. It builds on rc.3.2 (J1 soft `min_points`, J2 `〔1〕` labels, J3 `metrics.env`) and keeps the earlier fixes: plugin-declared settings rendered generically on both front ends, model dropdowns from `provider.models`, Enter-to-save on the TUI plugin page, full commands in permission prompts, and the display name **FeatherAgent** (binaries and paths unchanged). **Windows conclusions must come from your real-hardware retest**; Wine only verifies basic startup and does not prove named-pipe concurrency, ConPTY, proxy behavior, or real kill timing.

## 1. Unzip and start

Unzip into a writable directory (not `Program Files`):

```powershell
D:\FeatherAgent-1.2.1-rc.3.3\
```

First launch may take 1–2 minutes: Windows antivirus scans each plugin binary on first execution, and the kernel prints:

```text
[init] registering built-in plugin storage (first run may be scanned by antivirus)
[init] registering built-in plugin tool (first run may be scanned by antivirus)
...
```

Wait for `serve ready: 1.2.1-rc.3.3`, then continue.

```powershell
cd D:\FeatherAgent-1.2.1-rc.3.3
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

- `metrics.boot_mode` is `warm_in_process_same_install_root`; `boot_samples_ms` has 5 numbers, `boot_sample_count` is 5, `boot_ms_p90` is present; `boot_warmup_discarded_ms` is present.
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

v1.2.1-rc.2.1 regression checks:

1. Native Ollama: open Settings in the TUI and WebUI. The model list must populate from `GET /api/tags`; a `GET /models` 404 must no longer empty the list.
2. In the TUI `/settings`, verify all nine review fields are visible and reachable: enabled, max rounds, guide model/temp, exec model/temp, judge model/temp. Save with `Ctrl+S`, restart the TUI, and confirm the values reload from `config.json`. Resize the terminal small and confirm the selected field scrolls into view.
3. With a real 7B guide/judge, enable review-gate and send `REVIEW_START`. `review.guide` must contain `must_include`; `review.verdict.items[]` must carry `criterion` and `check`; passing evidence must quote at least 10 characters of the answer. Final `chat.finished.status` must be `ok` only when approved.
4. Force an unapproved review (or set `review_gate.max_rounds=1` with a model that cannot satisfy the rubric). `chat.finished.status` must be `unapproved` (or `degraded` when guide/judge fails twice), not `ok`.
5. Review with 4B only: if the model cannot produce a literal rubric, the turn must end `degraded`; it must not silently pass.

v1.2.1-rc.3 regression checks:

1. **Plugin-declared settings, both front ends.** Open the TUI `/settings` and the WebUI plugin page. Every plugin must appear in the sidebar under its own name. `review-gate` must show nine declared fields (enable, max rounds, guide model/temp, exec model/temp, judge model/temp, on-exhausted) and `search` must show its four; a plugin with no declaration must show “declares no settings” plus the enable toggle, not an empty form.
2. Change `max_rounds` to 3 in the TUI (`Ctrl+S`) and in the WebUI (Save plugin settings), then read `data/config.json`: both must land on `review_gate.max_rounds = 3`. The general settings page must **not** contain any review-gate or search fields any more.
3. Send a message after closing the settings page on both front ends: `model.message` and `chat.finished(ok)` must still arrive.
4. With a real 7B guide/judge, send a task whose answer is long. The rubric must contain 2–5 short criteria (no full sentences), at least one of them non-substring; `review.hardcheck` must report deterministic failures for banned phrases/point count/glob when violated.
5. Affirmative check on judge evidence: `review.verdict.items[].evidence` must be copied from the answer. Prompt echoes must trigger a judge retry, then `degraded` after two failures.
6. Tool name correction: ask the model to use a tool that does not exist (or feed `TOOL:{"name":"tool.fs"}`) and confirm `tool.error` returns `未知工具 tool.fs。可用工具：…`, with the turn continuing rather than the plugin tripping its breaker.
7. S3 reproducibility: run `tools/s3_batch.py` (or the same prompts by hand). t5 must call the read tool once before answering, and t6 must call it exactly once and then report the failure.

v1.2.1-rc.3.1 regression checks:

1. **Review-gate with a real 7B guide.** Send a normal task with review-gate enabled. `review.guide` must show a rubric whose items carry at least one of `must_include` / `must_not_include` / `min_points` / `pattern`; `criterion`/`check` alone is not a criterion. If the guide still returns descriptions only, the turn must show `review.warn` (“没有硬判据”) and still run the judge instead of ending `degraded`.
2. **Retry carries the error.** If the first rubric is rejected, `review.warn` must report “第 1 次判据不合格：…”, and the second guide answer must differ from the first (the kernel reinjects the error; it no longer resends the same prompt).
3. **Model dropdowns.** Open the review-gate page in WebUI and TUI. `guide.model`, `exec.model`, `judge.model` must be pickers filled from the local model service (`provider.models` / Ollama `/api/tags`), with a “（跟随当前模型）” entry; select one, save, and confirm `config.json`. Type a model the service does not report and confirm it also saves (custom entry must stay possible).
4. **Enter saves anywhere (TUI).** Open the plugin page, move the cursor to a value row (not the save row), press Enter, then reopen the page: the value must persist. Pressing Enter on the enable row must toggle and save.
5. **Permission prompt shows the command.** Trigger `tool.shell` (ask “run `echo hi`”). The TUI popup and the WebUI modal must show the complete command (`echo hi`), not only the tool name and risk. `n` / Esc must deny without executing.
6. **S3 t6 single call.** Run `tools/s3_batch.py`: t6 must call exactly one tool (the read) and then explain the failure; no `tool.shell`/`tool.ping` follow-up.
7. H3 entry without a cooperative model: `featheragent chat --text 'TOOL:{"name":"tool.fs","arguments":{"path":"a.txt"}}'` (provider-mock) must produce `tool.error` with `未知工具 tool.fs` and the available list.

v1.2.1-rc.3.2 regression checks:

1. **J1: `min_points` is soft.** With a 7B guide, ask for a structured answer and let the exec write three points in one paragraph separated by `；` or as `**bold headings**`. The round must not be rejected with “只有 1 个要点”; if the machine count is still short, `review.warn` must say “min_points 机器计数不足，交 judge 复核” and the judge must decide from the answer.
2. **J2: no rubric ids in the answer.** Force one failed review (e.g. rubric with `min_points:2` and a one-line answer). `review.feedback.text` must use `〔1〕` labels, never `- c1:`, and the revised answer must not contain `c1/c2/c3` headings.
3. **J3: bench fingerprint.** `featheragent.exe bench --json` must include `metrics.env` with `cpu_count`, `mem_total_kb`, `mem_available_kb`, `mem_load_pct` and `uptime_s`; the human-readable run must print one “环境指纹” line. Record it next to every bench number.
4. **Display name.** `featheragent.exe version`, `featheragent-tui.exe version`, `featheragent-webui.exe version` must print `FeatherAgent …` (TUI/WebUI included), while binary names remain lowercase.

v1.2.1-rc.3.3 regression checks:

1. **K1: negative criterion stays in hardcheck.** Enable review-gate with a real 7B. Send a task whose rubric contains `must_not_include` (e.g. “no TODO/FIXME”) plus one description item. The round must not produce `evidence 不是答案原文`; the negative item must be settled by `review.hardcheck` and only the description item may appear in `review.verdict`.
2. **K1: regex criterion.** A rubric item with `regex` and `min_matches` must be accepted by guide; an invalid regex must be rejected before the exec round. Set `hardcheck.pattern_engine=regex` and confirm a `pattern` item is interpreted as a regular expression.
3. **K2: missing item reinjected.** Force a judge omission (e.g. a 4B judge or a three-item description rubric). `review.warn` must say `第 1 票第 1 次审批不合格：…缺少验收项…`, the retry must list every id, and a compliant retry must end `approved` rather than `degraded`.
4. **Votes.** Set `judge.votes=3`, enable review-gate, and run three rounds. Approval must stay stable; the system must never run two model calls at once (watch the provider log timestamps). With `judge.early_stop_on_agreement=true`, two identical votes should skip the third.
5. **K3: warm tail.** `bench --json` must include `boot_ms_p90` and `boot_sample_count=5`; the human-readable line must print P50/P90. The gate still uses P50.
6. **K4: memory note.** `metrics.env` must include `mem_load_pct` and `reliability`. Under ≥85% memory load it must include the `note`; gates must not turn red because of it.
7. **Settings coverage.** The review-gate page must show 14 fields; `judge.votes=2` must be rejected, while 1/3, `criteria_routing`, and `pattern_engine` must save and reload on both TUI and WebUI.

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
- v1.2.1-rc.3.3 did not run Wine (rc.3, rc.3.1 and rc.3.2 did not either); passing Wine does not equal passing Windows.
- Still untested: high-DPI/Chinese rendering, real Ollama 7B/4B metrics, proxy environments, network/sync drives, 24h soak.

## 8. Artifact verification

```powershell
Get-FileHash .\FeatherAgent-1.2.1-rc.3.3-windows-x64.zip -Algorithm SHA256
```

Use the release assets and manifest for the authoritative byte count/hash. All five exes depend only on Windows system DLLs; the root contains `LICENSE`, `SECURITY.md`, `CONTRIBUTING.md`, and `CHANGELOG.md`.
