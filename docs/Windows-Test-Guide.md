# FeatherAgent 1.2.1-rc.4.2 · Windows x64 Test Guide

> Package under test: `FeatherAgent-1.2.1-rc.4.2-windows-x64.zip`
> Baseline: v1.1.9 real-hardware retest report (Q1/Q2), rc.4.1 retest (N15–N18)
> Note: v1.2.1-rc.4.2 fixes the four actionable findings from the rc.4.1 retest. N10, N12 and N13 were confirmed fixed; N11's plain-text form worked but the XML-style function/parameter variant still failed the whole turn, so N15 adds parser recovery plus an Ollama stream-error salvage path. N16 adds Windows pipe retries (`ERROR_FILE_NOT_FOUND(2)` / `ERROR_PIPE_NOT_CONNECTED(233)`) and a bounded `bench` connect retry. N17 moves `boot_gate_trusted` from `gates` to `advisory_gates`; it no longer counts in `passed/total`. N18 distinguishes default brevity from list/compare/enumerate tasks that must cover every point. The rc.4/rc.4.1 fixes N5–N14 remain in force. The package still contains ten exes (five Chinese + five `-en`). **Windows conclusions must come from your real-hardware retest**; Wine is only a startup smoke test.

## 1. Unzip and start

Unzip into a writable directory (not `Program Files`):

```powershell
D:\FeatherAgent-1.2.1-rc.4.2\
```

First launch may take 1–2 minutes: Windows antivirus scans each plugin binary on first execution, and the kernel prints:

```text
[init] registering built-in plugin storage (first run may be scanned by antivirus)
[init] registering built-in plugin tool (first run may be scanned by antivirus)
...
```

Wait for `serve ready: 1.2.1-rc.4.2`, then continue.

```powershell
cd D:\FeatherAgent-1.2.1-rc.4.2
.\featheragent.exe help
.\featheragent.exe serve --root . --provider provider-ollama --model qwen3.5:4b
```

In another terminal:

```powershell
.\featheragent-tui.exe --root .
.\featheragent-webui.exe --root . --app
.\featheragent.exe chat --root . --text "hello"
```

The default binaries are Chinese. For the English editions, run:

```powershell
.\featheragent-en.exe help
.\featheragent-tui-en.exe --root .
.\featheragent-webui-en.exe --root . --app
```

There is no `--lang` switch; the `-en` filename suffix selects the English kernel/TUI/WebUI assets. The `feather-plugin-en.exe` and `feather-storage-en.exe` workers are protocol-only aliases.

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

### File attachments and search artifacts

1. **WebUI `+`.** Click the `+` in the composer; the native file picker opens. Choose a small text file. A chip must appear with the kernel path; `file.attach` must return `copied:true` when the file is uploaded, and the path must be under `workspace\default\uploads\`. Send a message with the attachment; the model-visible text must contain `[附件] <that path>` (English: `[Attachment] <path>`).
2. **TUI `@`.** Send `@D:\temp\note.txt 请读取这个文件`; the TUI must show the resolved kernel path, and a missing path must fail before `chat.send`. Quote `@"D:\My Files\a.txt"` for spaces.
3. **Outside install root.** Pick a file outside the unpacked install directory; the kernel must copy it to `workspace\default\uploads\` and return the copy path. A file inside the install root may keep its path.
4. **Search artifact.** After `web.search`, `web.news` or `web.fetch`, the tool result must include `artifact`; `workspace\<project>\.featheagent\search\` must contain a JSON file with `tool`, `arguments`, `result`, `untrusted:true`.

## 5. Markdown and icons

- TUI: send `# heading`, `- list`, `1. ordered`, `> quote`, code block, `**bold**`, `*italic*`, `[link](https://…)`; the assistant reply should drop Markdown markers while keeping structure.
- WebUI: the same message renders headings, lists, quotes, code blocks, and clickable links; assistant messages no longer show raw `**` or triple backticks.
- Taskbar/program icons: all ten exes show the cyan feather icon; WebUI top-left and favicon show it too.

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

1. **J1: `min_points` is soft.** With a 7B guide, ask for a structured answer and let the exec write three points in one paragraph separated by `；` or as `**bold headings**`. The round must not be rejected with “只有 1 个要点”; if the machine count is still short, `review.warn` must say “min_points 机器计数不足，按软判据接受（确定性判据，不交 judge）”.
2. **J2: no rubric ids in the answer.** Force one failed review (e.g. rubric with `min_points:2` and a one-line answer). `review.feedback.text` must use `〔1〕` labels, never `- c1:`, and the revised answer must not contain `c1/c2/c3` headings.
3. **J3: bench fingerprint.** `featheragent.exe bench --json` must include `metrics.env` with `cpu_count`, `mem_total_kb`, `mem_available_kb`, `mem_load_pct` and `uptime_s`; the human-readable run must print one “环境指纹” line. Record it next to every bench number.
4. **Display name.** `featheragent.exe version`, `featheragent-tui.exe version`, `featheragent-webui.exe version` must print `FeatherAgent …` (TUI/WebUI included), while binary names remain lowercase.

v1.2.1-rc.4 regression checks:

1. **N5: match modes.** Force a guide with four `must_include` terms and an answer covering two. With the default (no `match`), hardcheck must pass (`{hits}/{total}` ≥ 2). Set `match:"all"` and it must fail; set `match:"any"` and one term must pass; `min_matches:3` must require three. Keep `["SYN","ACK"]` on the TCP task with `match:"all"`.
2. **N6: subjective must_not_include is rejected.** Use a task such as `用三句话说明什么是操作系统` and a criterion `使用准确的术语` plus `must_not_include:["软件","硬件","计算机"]`. Guide validation must reject it with a subjective-criterion message and retry. A banned word present in the user input must also reject the guide.
3. **N4 regression.** The rc.3.5 raw fixture item (`must_not_include:["REVIEW-FINAL","REVIEW-START","<...>"]`) must still be dropped by itself with `review.warn` naming its id.
4. **N7: bench temp/error contract.** Run `bench --json` several times; every run must have a distinct temp directory and no `os error 2`. If a path is deliberately broken, the JSON must be one line with `ok:false`, `code:"artifact_missing"` or `"bench_failed"`, and the path in `error`.
5. **N8: duplicate tool calls.** Trigger the retry chain with a model that repeats the same tool call. `tool.call` must carry `attempt` and `call_index`; identical name+arguments in one reply must execute once. A different path must still execute separately.
6. **N9: advisory boot gate.** Under memory pressure (`reliability` medium or low), `gates.boot_gate_status` must be `"advisory"`, `boot_gate_raw` must preserve the raw result, `gates.boot_lt_*` must not alone turn the run red, and `reliability` must appear in both `metrics` and `gates`; `boot_gate_trusted` must appear under `advisory_gates`, not inside `gates`.
7. **N1–N3 regression.** Keep the rc.3.6 checks: term-only `must_include` with connection-word rejection, P50 ≤ P90 with `metrics_consistent`, and empty/tool-format retries with `model.retry`.
8. **Bilingual package.** All ten exes must start. English CLI/TUI/WebUI must show English; Chinese editions must remain Chinese.

v1.2.1-rc.4.2 regression checks:

1. **N15: XML-style call recovery.** With review-gate on and a 4B exec, repeat the write task until the model emits a function/parameter-style call. The turn must execute `tool.fs.write`, write `workspace\notes.txt`, and finish successfully instead of `status=failed`; the raw XML must not leak into the final answer. If the model mixes prose and a call, record whether the parser recovered it.
2. **N16: bench connect.** Run `featheragent.exe bench --json` at least five times, and also with the host under memory pressure. There must be no `[bench:rpc_connect] ... 系统找不到指定的文件`; each result must have a unique `fa-bench-*` root and valid gates. If a failure still appears, keep the full path and OS error.
3. **N17: advisory split.** Inspect `bench --json`: `gates` must not contain `boot_gate_trusted`; `advisory_gates` must contain `boot_gate_trusted`, `boot_gate_status` and `reliability`. With `reliability` medium/low, the release gates may stay green and the advisory line must say it is not counted.
4. **N18: coverage.** Ask a list/compare question (for example, “列出三个要点并说明顺序和开销”). The answer must cover every requested point; a short answer that omits ordering or cost is a finding. Also verify the “默认简洁” behavior on a simple factual question.
5. **N10/N12/N13 regression.** Re-run the rc.4.1 checks: path `must_include`, `data\web-artifacts` audit copy, and `rubric-final.json`; all three were confirmed in rc.4.1 and must remain green.

v1.2.1-rc.4.1 regression checks:

1. **N10: path `must_include`.** Run the rc.4 write task that asks for `workspace/notes.txt`. The guide must accept `must_include:["workspace/notes.txt"]` (64-character path limit), the exec must run, and the round must reach `review.hardcheck`/judge instead of three guide failures. An intentionally overlong path (>64 characters) must be rejected with the `pattern/regex` hint.
2. **N11: plain-text fallback.** With review-gate on and a 4B exec, ask it to write a file. If it emits `tool_call tool.fs.write {...}` in the answer, the call must be executed as `tool.fs.write` rather than shown as final prose; `tool_fs_read` / `tool_fs_write` / `tool_shell` aliases must normalize. Feedback text must include the tool-format guard. This is a recovery path, not a guarantee; record any still-unrecognized name as a finding.
3. **N12: audit artifact.** After `web.search` / `web.news` / `web.fetch`, check both `workspace\<project>\.featheagent\search\` and `data\web-artifacts\<session>\`. The tool result must carry `artifact` and `audit_artifact`; both files must be valid JSON with `untrusted:true`. A deliberately read-only `data/` must produce `persist_error`, not fail the search.
4. **N13: final rubric.** After a guide succeeds, `data\review-artifacts\<session>\` must contain a `*-rubric-final.json` with `role:"rubric-final"` and normalized `match`/`min_matches`; `review.guide.final_artifact` must point at it. If all guide attempts fail, `rubric-final` must be absent while `guide-aN` files remain.
5. **N14: model boundary.** Run the multi-task prompt with a 4B model twice and a ≥7B model twice. Record per-run scores, call counts and wall time; do not treat a single 4B drift run as a kernel regression gate.

v1.2.1-rc.3.5 regression checks:

**M1/M3/M4 first:**

1. **M1: placeholder meta item is dropped, not retried forever.** Use a real 7B and a TCP-handshake task. If the guide writes a criterion such as “forbid the placeholder from the example” or puts `<...>` / `__FILL_*__` into `must_not_include`, `review.warn` must report `已丢弃占位符/示例判据`, the remaining real items must still be evaluated, and the round must not end `degraded`.
2. **M1: array `must_include`.** A rubric item with `must_include:["SYN","ACK"]` must be accepted and settled by `review.hardcheck`; an answer missing `ACK` must fail it, not silently pass as a description item.
3. **M3: subjective item is advisory.** For a task whose guide emits “判断答案是否简洁明了”, `review.warn` must say `主观判据`, the item must not appear in `review.verdict` item ids, and `review.verdict.votes` must be 0 when no other description item exists. The round must not swing solely because of that item.
4. **M4: t6 always calls the tool.** Run S3 t6 several times: it must call `tool.fs.read` once, report the missing file, and not answer from guesswork.

**Carried over from rc.3.4:**

1. **L1: guide writes task criteria, not schema text.** Use a real 7B and the plain task `用三句话说明什么是操作系统` (no `REVIEW_*` token). `review.guide` must succeed; the rubric must not contain `这项检查什么`, `怎么核对`, `REVIEW-FINAL`, or a literal `<...>`. The round must not fail every time on `c1`.
2. **L1: artifacts.** After that round, `data\review-artifacts\<session>\` must contain `*-guide-a*.json` and, if the judge ran, `*-judge-v*-a*.json`. Each file must be valid JSON with `role` guide/judge, `raw`, and `parse_error`. `review.guide.artifact` and `review.verdict.artifacts` must point at these files.
3. **L2: no literal in feedback.** Force a `must_include` failure. `review.feedback` must say the answer did not cover the requirement in the named criterion; it must not repeat the literal phrase, and the exec must not pass by appending that phrase.
4. **K1: negative criterion stays in hardcheck.** Enable review-gate with a real 7B. Send a task whose rubric contains `must_not_include` (e.g. “no TODO/FIXME”) plus one description item. The round must not produce `evidence 不是答案原文`; the negative item must be settled by `review.hardcheck` and only the description item may appear in `review.verdict`.
5. **K1: regex criterion.** A rubric item with `regex` and `min_matches` must be accepted by guide; an invalid regex must be rejected before the exec round. Set `hardcheck.pattern_engine=regex` and confirm a `pattern` item is interpreted as a regular expression.
6. **K2: missing item reinjected.** Force a judge omission (e.g. a 4B judge or a three-item description rubric). `review.warn` must say `第 1 票第 1 次审批不合格：…缺少验收项…`, the retry must list every id, and a compliant retry must end `approved` rather than `degraded`.
7. **Votes.** Set `judge.votes=3`, enable review-gate, and run three rounds. Approval must stay stable; the system must never run two model calls at once (watch the provider log timestamps). With `judge.early_stop_on_agreement=true`, two identical votes should skip the third.
8. **K3: warm tail.** `bench --json` must include `boot_ms_p90` and `boot_sample_count=5`; the human-readable line must print P50/P90. The gate still uses P50.
9. **K4: memory note.** `metrics.env` must include `mem_load_pct` and `reliability`. Under ≥85% memory load it must include the `note`; gates must not turn red because of it.
10. **Settings coverage.** The review-gate page must show 14 fields; `judge.votes=2` must be rejected, while 1/3, `criteria_routing`, and `pattern_engine` must save and reload on both TUI and WebUI.

v1.2.1 additional checks:

1. Settings shows `provider-llamacpp`, `provider-ninfer`, `provider-lmstudio`, `provider-vllm`, and `provider-openai`; changing provider fills the corresponding default base URL unless a custom URL is already present.
2. Start a local OpenAI-compatible server (llama.cpp `--server` or NInfer). Pick the matching provider, click the model-list refresh, chat, and confirm the reply comes from the local server.
3. Fill the optional `API Key` field; confirm the local server/proxy receives `Authorization: Bearer <key>`. Clear it and confirm the header is absent.
4. Enable `三段校验 review-gate` with `provider-mock`; send `REVIEW_START`. Expect `review.guide`, at least one failing `review.verdict`, `review.feedback`, a second `review.verdict` that passes, and final answer `REVIEW-FINAL` with `chat.finished.review.approved=true`.
5. Set guide/exec/judge temperatures outside 0.2–0.5 / 0.6–1.5 / 0.0–0.2; saving must be rejected.

## 7. Known boundaries

- N2 adds `[init]` progress but **does not remove** the first-run antivirus scan time.
- N14: 4B models can drift on multi-task long flows; use a ≥7B exec model and repeat the test baseline. Single 4B runs are observational.
- Old `<workspace_root>/<session>/` empty directories are not auto-cleaned; delete manually after confirming they are empty.
- WebUI Markdown/icons were verified in a real Linux browser; TUI Markdown is unit-tested only and still needs PTY confirmation.
- v1.2.1-rc.4.2 did not run Wine; passing Wine does not equal passing Windows.
- Still untested: high-DPI/Chinese rendering, real Ollama 7B/4B metrics, proxy environments, network/sync drives, 24h soak.

## 8. Artifact verification

```powershell
Get-FileHash .\FeatherAgent-1.2.1-rc.4.2-windows-x64.zip -Algorithm SHA256
```

Use the release assets and manifest for the authoritative byte count/hash. All ten exes depend only on Windows system DLLs; the root contains `LICENSE`, `SECURITY.md`, `CONTRIBUTING.md`, and `CHANGELOG.md`.
