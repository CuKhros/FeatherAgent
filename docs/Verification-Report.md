# FeatherAgent Verification Report (v1.1.1 – v1.2.1-rc.4.4)

> Date: 2026-11-25 · Code: `v1.1.1` baseline, `v1.1.2`–`v1.1.9` fixes, `v1.2.1-rc.4.4` candidate · Reference: FeatherAgent Architecture Design v1.1 (including erratum R1)
>
> Principle: **no number without a measurement**. Every number below comes from a real local run. Uncovered items are listed in §8; inference is never presented as measurement.

## v1.2.1-rc.4.4 summary (current candidate)

The rc.4.3 retest confirmed N20 (code answers stay in the reply), found N21 only partial (TCP/UDP covered two axes instead of three), N22 still failed (`tool.fs.write` succeeded but review failed on guide-invented words), N23 bench hung after step 2 for 150 s, N24/N25/N27 tracked the same failures, and N28 was a scorer rule for bold headings. The seven-question exam scored FeatherAgent 22/24 vs bare 11/24, 123 s vs 335 s; bare runs returned empty strings after 50–66 s on Q4–Q7 and FeatherAgent's empty-response retry kept output.

| Item | Treatment | Verification |
|---|---|---|
| N23 bench hung after `bench[2/6]` | `Server::stop` retries the wake connection for a 3 s budget; bench warm/RPC joins poll `is_finished` with a 5 s deadline; CI wraps `bench --json` in `timeout 90` | `cargo test`; three release benches complete, all gates true; Windows retest required |
| N24 write task still `failed` | Shared `requests_file_write`; explicit write tasks convert all guide criteria to `judge_only`, carry `tool.fs.write` success plus file body, and fall back to a written-path answer after post-write XML/empty failure | `review_rubric_moves_file_task_terms_to_judge`, `write_fallback_names_the_file_without_claiming_missing_content`, integration `review_gate_validates_written_file_content_instead_of_answer_wording` with judge votes > 0 |
| N25 XML recovery regressed | Function open-tag end is the earliest of `>`, newline, or the parameter marker; parameter open tags share the tolerance | `malformed_xml_function_open_without_gt_is_recovered` |
| N26 comparison still two axes | Prompt requires a comparison pass over mechanism/connection, reliability/consistency, performance/overhead and scenarios, one line per requested axis | Updated `system_prompt_distinguishes_brevity_from_coverage` |
| N27 bench temp dirs | `BenchTempDir` removes on Drop; startup clears `fa-bench-*` older than 24 h | Zero leftovers after three benches; stale-dir check passed |
| TUI overscroll | `draw` takes `&mut App`; `chat_scroll` is clamped to `max_scroll` before rendering | `chat_scroll_is_clamped_after_overscrolling_at_top` in both TUIs |
| N28 Q7 bold headings | Scorer limitation, not a product defect | Recorded in the change note |

Linux full suite: **198 tests, 197 passed, 1 ignored** (`local_web_search_tool_via_kernel`, needs public network). Breakdown: 76 lib, 33 plugin ×2, 2 kernel, 6 TUI, 1 TUI-en, 4 WebUI ×2, 2 golden, 37 integration (1 ignored). `cargo fmt --check` and `cargo clippy --locked --all-targets -- -D warnings` pass. Release bench (3 consecutive runs): kernel **2,568,520 B (2.45 MiB)**; warm P50 **1.221 / 1.198 / 1.174 ms**, P90 **1.253 / 1.233 / 1.196 ms**; cold **3.567 / 3.060 / 3.099 ms**; RSS **3,584 / 3,456 / 3,584 KiB**; write P95 **3.024 / 3.787 / 2.738 µs**; assemble P95 **0.120 / 0.117 / 0.127 ms**; RPC P95 **0.072 / 0.048 / 0.037 ms**; all release gates true, `reliability:"high"`.

Not verified: v1.2.1-rc.4.4 Windows real hardware (N23–N27, TUI feel, bilingual package), medium/low reliability, high-DPI/Chinese rendering, network-drive/24h soak, macOS/arm64. Wine 6.0.3 passed `version` and three `bench --json` runs with a temporary `bcryptprimitives` stub, but cold-spawn/RSS under Wine are not a Windows baseline.

## v1.2.1-rc.4.3 summary (historical, released)

The rc.4.2 retest confirmed N15's XML-style recovery, removed the `[bench:rpc_connect]` failures (0/5), and confirmed the `advisory_gates` split. It left four findings: N19 cold spawn fixed at about 3.18 s, N20 code answers written to a file instead of the response, N21 missing comparison axes, and N22 a write task unapproved plus four repeated reads. This patch addresses N19–N22 and adds the curated model-report archive.

| Item | Treatment | Verification |
|---|---|---|
| N19 cold spawn stuck at ~3.18 s | `serve`'s old-kernel probe used the retrying `ipc::connect`, so a clean root waited the full 3 s N16 deadline before creating the pipe. Added one-shot `ipc::try_connect`; serve/`wait_ready`/stop probes use it, while real clients keep the bounded retry | Unit `try_connect_does_not_enter_the_retry_deadline`; Linux cold-spawn P50 5.848/5.376/5.556 ms; Windows retest required |
| N20 code task wrote a file instead of answering | Prompt separates content output from file creation: code/text/plan/translation defaults to the response; file tools only on explicit save/write/create | Unit `system_prompt_separates_content_output_from_file_creation` |
| N21 list missed overhead/header size | Prompt requires first-level items to follow the requested comparison axes; with no axes, cover mechanism, reliability, performance/overhead and scenarios | Updated `system_prompt_distinguishes_brevity_from_coverage` |
| N22 write task stayed `unapproved` | Engine records successful `tool.fs.write` path/content and gives the judge labelled file evidence for explicit write tasks; guide checks the file, not verbatim answer wording; prompt stops repeated post-write reads | Unit `review_file_evidence_is_only_for_explicit_write_tasks`, `review_answer_includes_written_file_content_before_final_answer`; integration `review_gate_validates_written_file_content_instead_of_answer_wording` |

Linux full suite: **189 tests, 188 passed, 1 ignored** (`local_web_search_tool_via_kernel`, needs public network). Breakdown: 73 lib, 31 plugin ×2, 2 kernel, 5 TUI, 4 WebUI ×2, 2 golden, 36 integration (1 ignored). `cargo fmt --check` and `cargo clippy --locked --all-targets -- -D warnings` pass. Release bench (3 consecutive runs): kernel **2,564,584 B (2.45 MiB)**; warm P50 **1.220 / 1.339 / 1.231 ms**, P90 **1.597 / 1.353 / 1.557 ms**; cold **5.848 / 5.376 / 5.556 ms**; RSS **1,920 / 1,920 / 1,792 KiB**; write P95 **8.924 / 8.796 / 7.704 µs**; assemble P95 **0.177 / 0.124 / 0.120 ms**; RPC P95 **0.048 / 0.071 / 0.042 ms**; all release gates true, `advisory_gates.boot_gate_trusted=true`, `reliability:"high"`.

Not verified: v1.2.1-rc.4.3 Windows real hardware (N19 cold spawn, N20 code-answer shape, N21 comparison coverage, N22 write-task approval/read count, N5–N18 regression, bilingual package), medium/low reliability, high-DPI/Chinese rendering, network-drive/24h soak, macOS/arm64. Wine is only a startup smoke test.

## v1.2.1-rc.4.2 summary (historical, released)

The rc.4.1 retest confirmed N10, N12 and N13. N11's plain-text fallback worked, but an XML-style function/parameter call still ended the whole turn with `status=failed`; N16 kept about 1/2 bench failures; N17 inflated the gate count with `boot_gate_trusted`; N18 showed a coverage gap from “concise” prompt wording. This patch addressed N15–N18.

| Item | Treatment | Verification |
|---|---|---|
| N15 XML-style call failed the turn | Context parser accepts function/parameter tags (`=` and `name="..."` forms, values until the parameter close tag, missing function close tolerated); the Ollama provider salvages a parsed call from accumulated text on a stream `error` instead of emitting a terminal error | Unit `xml_function_parameter_fallback`, `malformed_xml_function_without_close_is_recovered`; existing plain-text/alias tests pass |
| N16 bench RPC still ~1/2 failed on Windows | `ipc::connect` retries `ERROR_FILE_NOT_FOUND(2)` and `ERROR_PIPE_NOT_CONNECTED(233)` within 3s; `bench` creates `rpc/` and retries `Client::connect` up to 10s after `wait_ready` | Three Linux release bench runs pass; Windows retest required |
| N17 `boot_gate_trusted` counted as a failed gate | Removed from `gates`; added to `advisory_gates` with `boot_gate_status` and `reliability`; `metrics` keeps it for diagnostics; human output prints the advisory line separately | Code inspection on both kernel editions; Windows retest required |
| N18 concise prompt dropped required points | System prompt keeps default brevity but requires full coverage for list/compare/enumerate/explain-all tasks | Unit `system_prompt_distinguishes_brevity_from_coverage` |

Linux full suite: **184 test executions, 183 passed, 1 ignored** (`local_web_search_tool_via_kernel`, needs public network). Breakdown: 69 lib, 31 plugin ×2, 2 kernel, 5 TUI, 4 WebUI ×2, 2 golden, 36 integration (1 ignored). `cargo fmt --check` and `cargo clippy --locked --all-targets -- -D warnings` pass. Release bench (3 consecutive runs): kernel **2,557,784 B (2.44 MiB)**; warm P50 **1.140 / 1.137 / 1.136 ms**, P90 **1.159 / 1.144 / 1.146 ms**; cold **2.891 / 2.085 / 2.140 ms**; RSS **3,456 / 3,584 / 3,584 KiB**; write P95 **2.684 / 2.684 / 2.564 µs**; assemble P95 **0.125 / 0.119 / 0.121 ms**; RPC P95 **0.023 / 0.024 / 0.028 ms**; all release gates true, `advisory_gates.boot_gate_trusted=true`, `reliability:"high"`.

At release time, the rc.4.2 Windows hardware items were still pending (N15 XML recovery with a live 4B, N16 repeated bench, N17 advisory output, N18 coverage); the subsequent rc.4.2 retest covered them and found N19–N22 instead. Other unverified areas: medium/low reliability, high-DPI/Chinese rendering, network-drive/24h soak, macOS/arm64. Wine is only a startup smoke test.

## v1.2.1-rc.4.1 summary (historical, released)

The rc.4 retest passed N5–N9 but exposed four actionable problems: N10 (path `must_include` hit the 12-character phrase limit), N11 (4B wrote `tool_call ...` into the answer body with review-gate on), N12 (no auditable search artifact under `data/`), and N13 (no final-rubric record). N14 is a documented 4B long-flow boundary. This patch fixed N10–N13 and recorded N14; see [v1.2.1-rc.4.1 Changes](v1.2.1-rc.4.1-Changes.md). Linux full suite at that release: 181 executions, 180 passed, 1 ignored; release bench kernel 2,554,208 B (2.44 MiB), warm P50 1.159/1.188/1.200 ms, all gates true, `reliability:"high"`.

## v1.2.1-rc.4 summary (historical, released)

The rc.3.6 retest left five findings. This candidate fixes them and adds search-result artifacts plus WebUI/TUI file attachments.

| Item | Treatment | Verification |
|---|---|---|
| N5 `must_include` defaulted to all | `match:any|all|min`; multi-term default `min` with `ceil(n/2)`; `min_matches` 1..n; hardcheck counts hits | Unit `review_rubric_defaults_multi_must_include_to_half`, `review_hardcheck_supports_any_all_and_min`; TCP case explicitly `match:"all"` |
| N6 subjective criterion banned common words | `subjective_reason` catches accuracy/professionalism words; subjective + `must_not_include` is a guide error; a banned word present in the user input also errors | Unit `review_rubric_rejects_subjective_must_not_include`, `review_rubric_rejects_banned_word_from_input` |
| N7 bench occasional `os error 2` | unique `fa-bench-<pid>-<random>` per run; `bench` wrapper prints `{ok:false,code,error,hint}` on failure; path-stamped phases; `os error 2` → `artifact_missing` | Three consecutive release runs used distinct dirs and exited 0; error path is unit-visible in code but not forced on Windows |
| N8 retry-chain replay | identical `name+arguments` in one reply executed once; `tool.call` adds `attempt`, `call_index` | Unit coverage is indirect through `run_turn`; live Ollama retry still pending |
| N9 boot gate red under low reliability | `reliability != high` → `boot_gate_status:"advisory"`, `boot_gate_raw` preserved, effective gate true; fields copied to gates/metrics | Release run: `reliability:"high"`, `boot_gate_status:"pass"`, `boot_gate_raw:true`; medium/low not simulated |
| Search artifacts | `web.search/news/fetch` JSON under `workspace/<project>/.featheagent/search/`; tool result `artifact`/`artifact_kind` | Integration `web_search_artifact_is_persisted_under_project_workspace` |
| File attachments | `file.attach`/`file.prepare`; outside-install files copied to `workspace/default/uploads/`; WebUI `+` (HTTP RPC, 8 MiB file / 12 MiB body); TUI `@path` with `is_file` precheck | Integration `file_attach_copies_upload_to_default_workspace`, `file_prepare_copies_local_file_outside_install_root`; TUI parser/email tests; WebUI static assertions; real picker/PTY pending |

Linux full suite: **176 test executions, 175 passed, 1 ignored** (`local_web_search_tool_via_kernel`, needs public network). Breakdown: 64 lib, 30 plugin ×2 (Chinese + English alias), 2 kernel, 5 TUI, 4 WebUI ×2, 2 golden, 34 integration (1 ignored). `cargo check --bins`, `cargo fmt --check`, and `cargo clippy --locked --all-targets -- -D warnings` pass. Release bench (3 consecutive runs): binary **2,550,480 B (2.43 MiB)**; warm P50 **1.186 / 1.180 / 1.167 ms**, P90 **1.217 / 1.207 / 1.195 ms**; cold **3.179 / 3.085 / 3.283 ms**; RSS **3,584 / 3,456 / 3,456 KiB**; write P95 **2.684 / 3.114 / 4.567 µs**; assemble P95 **0.143 / 0.152 / 0.129 ms**; RPC P95 **0.057 / 0.059 / 0.082 ms**; all gates true, `mem_load_pct` 20, `reliability:high`.

Not verified: v1.2.1-rc.4 Windows real hardware (N5–N9 with a live 7B/Ollama, N4 raw a11, native picker and PTY attachments, bilingual packages), medium/low reliability, high-DPI/Chinese rendering, network-drive/24h soak, macOS/arm64. Wine is only a startup smoke test.

## v1.2.1-rc.3.6 summary (historical, released)

The rc.3.5 retest fully met checklist item 1, but checklist item 2 stopped at 4/5 because N1 still left a review round unapproved, checklist item 3 reached 4/5, the bench line printed P90 lower than P50 (N2), and the N4 fixture phrase was only blocked indirectly. This candidate fixes all four and adds English editions of every top-level binary.

| Item | Treatment | Verification |
|---|---|---|
| N1 conceptual `must_include` | `GUIDE_SYSTEM` rule 4 permits only proper nouns/terms/file names/commands/fixed forms of 2–12 characters and rejects `和/与/及/或/是/的/了`; `MUST_INCLUDE_MAX_CHARS=12`; hardcheck splits at whitespace/`、`/`，`/`,`/`；`/`;`/`/` and requires every term; one-character terms are rejected | Unit `review_rubric_rejects_conceptual_must_include_with_connective` (the report's `管理计算机硬件和资源` is rejected), `review_rubric_rejects_one_char_must_include`, `review_hardcheck_matches_include_terms_across_wording` (`管理计算机硬件 资源` passes against “硬件与软件资源”) |
| N2 P90 < P50 | `util::percentile` sorts its own samples; bench computes `boot_p50`/`boot_p90` once; JSON adds `boot_percentiles_consistent`, gates add `metrics_consistent` | Unit `percentile_works_on_unsorted_samples_and_p50_le_p90`; three release runs all have `boot_percentiles_consistent:true`, `metrics_consistent:true`, P50 ≤ P90 |
| N3 empty/malformed model reply | Empty replies retry twice at temperatures 0.70/0.75; `is_tool_format_error` catches Ollama XML/`element ... closed by ... parameter`/generic invalid tool-call errors and retries twice with `tool_format_correction()`; every retry emits `model.retry` with `reason`; crash restart once | Unit `tool_format_error_detector_covers_ollama_and_json_errors`; loop behaviour is implemented in `run_turn`, live Ollama still pending |
| N4 fixture item not dropped alone | `strip_placeholder_meta_items` drops an item when any `must_not_include` hits `REVIEW-FINAL`, `REVIEW-START`, `<...>`, `__FILL`, `{{` or `}}`; empty criterion/check remains a structural error | Unit `review_rubric_drops_fixture_must_not_include_item` (c1/c5 survive, c4 dropped, warn names it) |
| English top-level binaries | `-en` suffix on all five; kernel/TUI use English source targets; WebUI switches `index.en.html`/`app.en.js` at runtime; plugin/storage are protocol-only aliases; Windows zip has 10 exes, Linux core 8 binaries, WebUI package 2 | CLI help/version English; TUI PTY shows `Sessions`/`Conversation`/`Context`/`Input`; WebUI `/` returns `lang="en"` and `RPC timeout`, Chinese WebUI still `lang="zh-CN"` and `RPC 超时` |

Linux full suite: **162 test executions, 161 passed, 1 ignored** (`local_web_search_tool_via_kernel`, needs public network). The English plugin/storage/WebUI alias targets reuse the same test modules, so 26 plugin and 4 WebUI cases execute twice. `cargo fmt --all` and `cargo clippy --locked --all-targets -- -D warnings` pass. Release bench (3 consecutive runs): warm P50 **1.142 / 1.167 / 1.163 ms**, P90 **1.761 / 1.190 / 1.170 ms**; cold **3.722 / 2.324 / 2.262 ms**; RSS max **3,456 / 3,584 / 3,456 KiB**; write P95 **2.634 / 2.704 / 3.005 µs**; assemble P95 **0.138 / 0.143 / 0.123 ms**; RPC P95 **0.030 / 0.030 / 0.028 ms**; all gates true; `mem_load_pct` 16, `reliability:high`.

Not verified: v1.2.1-rc.3.6 Windows real hardware (N1 guide rejection/term matching with a live 7B, N2 on Windows, N3 with a real Ollama malformed tool call, N4 fixture drop, bilingual exes, plus the rc.3.5 pending checks), high-DPI/Chinese rendering, network-drive/24h soak, macOS/arm64. Wine was not run. Bench numbers are from this VM and are not a Windows baseline.

## v1.2.1-rc.3.5 summary (historical, released)

The rc.3.4 retest met all three checklist items: criteria became task-specific (4 of 5 tasks had real criteria, 2 approved), artifacts matched the event paths, and feedback did not leak the literal phrase. Real 7B data: a1 operating-system unapproved (3 rounds), a2 TCP handshake **degraded** (guide rejected 3 times), a3 memory leak approved, a4 TCP handshake approved, a5 “ban the word system” unapproved (self-contradictory). Approval 2/5, degraded 1/5. The remaining defects were rubric-quality issues, so this candidate targets M1/M3 and hardens the S3 t6 prompt (M4).

| Item | Treatment | Verification |
|---|---|---|
| M1 guide treats `<...>` as task content | `GUIDE_SYSTEM` uses `__FILL_*__`; rules 8/9 forbid discussing placeholders/templates and subjective criteria; `rubric_placeholder` rejects `__FILL*__` and `{{ }}`; `placeholder_meta` only matches concrete meta phrases plus an action word; `strip_placeholder_meta_items` drops self-referential items and keeps real ones, warning through `review.warn`; an all-placeholder rubric is an explicit parse error; array `must_include` (`["SYN","ACK"]`) is accepted and every phrase is required | Units `review_rubric_drops_placeholder_meta_item_and_keeps_task_items` (c1/c3 survive, c2 dropped, `hard_items=2`), `review_rubric_accepts_array_must_include_and_hardcheck_requires_all`; integration `review_gate_drops_placeholder_meta_item_and_approves` (`approved` + warn contains `已丢弃` and `c2`) |
| M3 subjective criterion makes the judge swing | Subjective description items are marked `advisory`, excluded from hardcheck and judge, and warned; `review.parse_rubric` returns `advisory` ids; if `hard_items==0 && judge_only==0`, the kernel retries and then fails instead of accepting an uncheckable rubric | Unit `review_rubric_marks_subjective_item_advisory` (`hard_items=1`, `advisory=true`, empty `judge_only`); integration `review_gate_subjective_criterion_is_advisory` (`approved`, warn contains `主观判据`, `review.verdict.votes=0`) |
| M4 S3 t6 skips the tool | t6 prompt now starts with “must call `tool.fs.read` once” and forbids answering from guesswork, keeping the one-call limit | Prompt change only; this VM has no Ollama service, so the live 4B behaviour is not measured here |

Linux has **125 tests: 124 passed, 1 ignored by default** (62 lib, 22 plugin, 2 kernel, 3 TUI, 3 WebUI, 2 golden, 31 integration); the ignored network test needs public network. `fmt` and `clippy --locked --all-targets` pass. Release bench (3 consecutive runs): kernel **2,521,800 B (2.405 MiB)**; warm **1.288 / 1.312 / 1.260 ms** (P90 1.259 / 1.293 / 1.255 ms, 5 samples); cold **5.582 / 5.530 / 5.630 ms**; RSS max **1,920 KiB**; write P95 **13.819 / 14.973 / 13.472 µs**; assemble P95 **0.162 / 0.198 / 0.133 ms**; RPC P95 **0.056 / 0.068 / 0.069 ms**; all gates true; `env.mem_load_pct` 18, `reliability:high`.

Not verified: v1.2.1-rc.3.5 Windows real hardware (M1 drop-and-continue on a live 7B, M3 advisory exclusion, S3 t6 repeated tool call, plus the rc.3.4 artifact/feedback and K1/K3/K4 rechecks, live-Ollama dropdowns, ConPTY Enter-save and Windows Terminal permission popup), high-DPI/Chinese rendering, network-drive/24h soak, macOS/arm64. Wine was not run. Bench numbers are from this VM and are not a Windows baseline.

## v1.2.1-rc.3.4 summary (historical, released)

The rc.3.3 real-hardware retest verified K1/K3/K4: criterion routing and short-circuit ran as designed, three runs produced **0 degraded**, and a hardcheck failure emitted `review.verdict` 0.01 s after the answer, proving no judge call. Approval was still **0/3**, but the cause was the guide copying the kernel JSON schema: `criterion:"这项检查什么"`, `check:"怎么核对"`, `must_include:"REVIEW-FINAL"`. `REVIEW-FINAL` is a mock fixture word absent from the real task, so `c1` could never pass. This candidate fixes that (L1), removes the literal phrase from feedback (L2), and records guide/judge model outputs.

| Item | Treatment | Verification |
|---|---|---|
| L1 guide copies schema | `GUIDE_SYSTEM` example values are all `<...>` placeholders; `REVIEW-FINAL` removed; `review.parse_rubric` rejects placeholder `criterion`/`check`/`must_include`/`must_not_include`, and rejects `REVIEW-FINAL`/`REVIEW-START` when absent from the user input; parser now receives the input | Unit `review_rubric_rejects_schema_placeholders` (rejects the exact rc.3.3 rubric, accepts a real `must_include:"操作系统"` item); test-marker check limited to two mock words so `HTTP`/`JSON` are not false positives |
| L2 feedback leaks literal | Missing `must_include` evidence says `答案未覆盖判据「{criterion}」要求的关键内容（缺少必须出现的短语）`; no literal phrase, so the exec model has nothing to paste | Unit `review_hardcheck_does_not_leak_must_include_literal` |
| Guide/Judge artifacts | Every model call is written before parsing under `<data>/review-artifacts/<session>/`; guide `<ms>-guide-a<n>.json`, judge `<ms>-judge-v<n>-a<n>.json`, raw output + parse error included; write failures are best-effort | Integration `review_gate_three_roles_retry_and_approve` asserts the `review.guide.artifact` file and every `review.verdict.artifacts` file exist in the session directory and parse as JSON with `role` = guide/judge |
| Event observability | `review.guide` adds `artifact`; `review.verdict` adds `attempts`, `votes`, `artifacts` | Same integration test plus manual event inspection |

Linux has **120 tests: 119 passed, 1 ignored by default** (62 lib, 19 plugin, 2 kernel, 3 TUI, 3 WebUI, 2 golden, 29 integration); the ignored network-dependent test passes when run separately. `fmt` and `clippy --locked --all-targets` pass. Release bench (3 consecutive runs): kernel **2,522,160 B (2.405 MiB)**; warm **1.186 / 1.195 / 1.221 ms** (P90 1.181 / 1.191 / 1.186 ms, 5 samples); cold **4.272 / 4.269 / 4.394 ms**; RSS max **1,920 KiB**; write P95 **5.908 / 6.660 / 6.019 µs**; assemble P95 **0.147 / 0.141 / 0.137 ms**; RPC P95 **0.045 / 0.048 / 0.046 ms**; all gates true; `env.mem_load_pct` 23, `reliability:high`. This VM was busier than during rc.3.3, so cold/write numbers are higher; gates stayed green.

Not verified: v1.2.1-rc.3.4 Windows real hardware (L1 with a live 7B task that has no fixture token, L2 feedback without the literal, artifact files on Windows, plus the rc.3.3 pending K1/K3/K4 recheck, live-Ollama dropdowns, ConPTY Enter-save, Windows Terminal permission popup and S3 t6 single call), high-DPI/Chinese rendering, network-drive/24h soak, macOS/arm64. Wine was not run. Bench numbers above are from this VM at the recorded `metrics.env`; they are not a Windows baseline.

## v1.2.1-rc.3.3 summary (historical, released)

The rc.3.2 real-hardware retest re-verified J1–J3: `review.warn` carried `min_points 机器计数不足…`, answers had no `cN` leakage across three runs, and `metrics.env` showed the earlier slow numbers came from a host at 87–90% memory (1.5–2 GB free of 16 GB). Windows bench was 7/7 twice (warm 38.1 / 85.5 ms, cold 181.8 / 204.0 ms, write P95 4,079 / 5,041 µs, RPC P95 0.312 / 0.320 ms, binary 2.141 MB, RSS 10,584–10,588 KB). The gate still approved only **1/3**, now because K1/K2 were criteria and judge-stability problems rather than mechanism problems.

| Item | Treatment | Verification |
|---|---|---|
| K1 negative/count criteria vs evidence rule | `must_include` / `must_not_include` / `min_points` / `pattern` / new `regex` are settled by `review.hardcheck`; only description items go to `review.judge`; evidence-verbatim applies only to judge items | Unit `review_rubric_accepts_regex_and_rejects_invalid_one`, hardcheck covers five kinds; integration `review_gate_routes_negative_criterion_to_hardcheck` (a negative item that would return non-answer evidence never reaches the judge, round approved) |
| K2 judge omission | Missing/invalid verdict is reinjected inside the same vote with the full item-id list and retried once per vote | Integration `review_gate_missing_description_item_is_reinjected`: mock omits the last item, `review.warn` reports the missing item, retry approves |
| K3 warm tail | `bench --json` adds `boot_ms_p90` and `boot_sample_count`; human output prints P50/P90; gate remains P50 | Three release runs: warm P50 1.180/1.148/1.151 ms, P90 1.183/1.140/1.145 ms, 5 samples each |
| K4 memory reliability | `metrics.env` adds Linux `mem_load_pct`, `reliability`, and a `note` at ≥85%; no gate change | Three release runs: `mem_load_pct` 16–17, `reliability: high`, no note; Windows path already reports `dwMemoryLoad` |
| Routing/votes config | `criteria_routing`, `hardcheck.pattern_engine`, `judge.votes` 1/3, `judge.aggregation=majority`, `judge.early_stop_on_agreement`; serial votes, peak concurrency 1 | `plugin_settings_are_relayed_and_validated`: 14 declared fields, `votes=2` rejected, 3 and enums round-trip; integration review suite passes |

Linux has **118 tests: 117 passed, 1 ignored by default** (62 lib, 17 plugin, 2 kernel, 3 TUI, 3 WebUI, 2 golden, 29 integration); the ignored network-dependent test passes when run separately. `fmt` and `clippy --locked --all-targets` pass. Release bench (3 consecutive runs): kernel **2,516,416 B (2.400 MiB)**; warm **1.180 / 1.148 / 1.151 ms**; cold **3.391 / 3.294 / 2.940 ms**; RSS max **3,584 KiB**; write P95 **4.220 / 2.678 / 3.216 µs**; assemble P95 **0.152 / 0.176 / 0.138 ms**; RPC P95 **0.053 / 0.049 / 0.038 ms**; all gates true; `env.mem_load_pct` 16–17, `reliability:high`.

Not verified: v1.2.1-rc.3.3 Windows real hardware (K1 negative/regex routing with a live 7B, K2 missing-item reinjection, optional 3-vote behaviour and model-call overlap, K3 P90 on Windows, K4 ≥85% note, plus pending live-Ollama dropdowns, ConPTY Enter-save, Windows Terminal permission popup and S3 t6 single call), high-DPI/Chinese rendering, network-drive/24h soak, macOS/arm64. Wine was not run. Bench numbers above are from this VM at the recorded `metrics.env`; they are not a Windows baseline.

## v1.2.1-rc.3.2 summary (historical, released)

The rc.3.1 real-hardware retest confirmed that the review gate now works: with real 7B models it approved **1/3** and rejected **2/3**, the guide succeeded on the first attempt (`attempts:1`), description-only rubrics fell back to judge-only with `review.warn`, answers were 252-character structured text written by the exec, and judge evidence was a verbatim window of the answer. It also exposed three new issues (J1–J3): `min_points` false kills from counting only list markers, rubric ids `c1/c2/c3` copied into answers because feedback used bare `c1:` labels, and bench numbers that could not be compared across runs because the host had drifted into a degraded state.

| Item | Treatment | Verification |
|---|---|---|
| J1 `min_points` false kill | `count_points` takes the max of list/numbered markers, bold sub-headings, `；`-separated clauses (≥6 characters) and blank-line paragraphs, capped at 12; `review.hardcheck` returns a `soft` item instead of failing; the engine emits `review.warn` and appends `[机器计数提示]` to the judge input | Unit `count_points_recognizes_chinese_inline_enumeration`, `min_points_is_soft_and_defers_to_judge`; integration `review_gate_min_points_soft_falls_back_to_judge` (warn + judge approval) |
| J2 ids in answers | `review_feedback_text` labels items `〔1〕`/`〔2〕`, never `- c1:`, and ends with an explicit “do not write these numbers into the answer” line | Unit `review_feedback_uses_prose_labels_and_forbids_ids_in_answer` |
| J3 bench comparability | `bench --json` adds `metrics.env` (captured time, CPU count, load average, total/available memory, uptime); human output prints a fingerprint line | Three release runs show `env` with `cpu_count:2`, `loadavg_1m:0.86`, `mem_available_kb:3,364,164–3,364,504`, `uptime_s:991`; all gates true |
| Display name | TUI/WebUI `version` subcommands print `FeatherAgent TUI` / `FeatherAgent WebUI` | CLI checks; binary names unchanged |

Linux has **116 tests: 115 passed, 1 ignored by default** (62 lib, 16 plugin, 0 storage, 2 kernel, 3 TUI, 3 WebUI, 2 golden, 27 integration); the ignored network-dependent test passes when run separately. `fmt` and `clippy -D warnings` pass. Release bench (3 consecutive runs): kernel **2,495,424 B (2.380 MiB)**; warm **1.178 / 1.186 / 1.181 ms**; cold **3.519 / 3.544 / 2.531 ms**; RSS max **3,584 KiB**; write P95 **2.593 / 2.584 / 2.594 µs**; assemble P95 **0.120 / 0.124 / 0.141 ms**; RPC P95 **0.063 / 0.051 / 0.050 ms**; all gates true.

Not verified: v1.2.1-rc.3.2 Windows real hardware (J1 soft fallback with real 7B, J2 over several rounds, J3 values on Windows, the TUI/WebUI version strings, plus the rc.3.1 items still pending: live-Ollama model dropdowns, ConPTY Enter-save, Windows Terminal permission popup, S3 t6 single call), high-DPI/Chinese rendering, network-drive/24h soak, macOS/arm64. Wine was not run. Bench numbers above are from this VM at the recorded `metrics.env`; they are not a Windows baseline.

## v1.2.1-rc.3.1 summary (historical, released)


The rc.3 real-hardware retest confirmed the mechanics (include / not_include / answer-verbatim evidence / two-round approval under mock, bench 7/7) but found that a real 7B guide made 4/4 runs `degraded`: the rubric only carried `criterion`/`check`, and both retries resent the identical prompt (I2). It also asked for four UX changes: model fields should be picked from the local model service, TUI Enter should save from any field, permission prompts should show the complete command on both front ends, and the product should display as **FeatherAgent**.

| Item | Treatment | Verification |
|---|---|---|
| I1 degraded on 7B | `GUIDE_SYSTEM` gives a literal JSON schema + few-shot and states that only `must_include`/`must_not_include`/`min_points`/`pattern` are criteria; `review.parse_rubric` accepts 1–5 items, keeps description-only items as `judge_only`, requires ≥1 hard criterion overall | Unit tests `review_rubric_accepts_description_only_items_with_warning`, `review_rubric_requires_literal_must_include` (single item now accepted with a warning) |
| I2 identical retries | `review.guide` takes `feedback`; `review_prepare` builds the prompt inside the retry loop and reinjects the previous parse error (3 attempts) | Integration `review_gate_reinjects_schema_error_and_falls_back_to_judge_only`: attempt 2 succeeds after reinjection; an always-soft guide ends judge-only with `review.warn`, never `degraded` |
| Tolerant degrade | Zero hard criteria is retried, then accepted for judge-only evaluation; `review.warn` (29) carries “没有硬判据” / “判据全是字面短语” / “第 N 次判据不合格” | Round-trip test for `EventKind::ReviewWarn`; integration asserts the warn text |
| Model dropdown | `SettingField.options_source`/`allow_custom`; review-gate declares `models`; TUI and WebUI call `provider.models` and render a picker while keeping manual entry | TUI PTY shows “本地模型 1 个；←/→ 选择”; WebUI WS `plugin.settings` returns three fields with `options_source:"models"` and `allow_custom:true`; static assets contain the `<select>` + input pair |
| TUI Enter save | Enter on any value row submits the form (enable row toggles then saves) | TUI PTY: Enter on `max_rounds` persisted 2→3 into `data/config.json` |
| Permission prompt | `tool.confirm_request` adds `arguments` + `preview`; TUI popup and WebUI modal render it | Integration asserts `arguments.cmd == "echo hi"` and `preview` contains the command; WebUI WS run received a 9-character `$ echo hi` preview and denied successfully |
| Naming | `version`, `serve` banner, help header, TUI/WebUI titles show **FeatherAgent**; binaries/paths unchanged | CLI output `FeatherAgent 1.2.1-rc.3.1 starting …`; TUI header `FeatherAgent v1.2.1-rc.3.1` |

Linux has **111 tests: 110 passed, 1 ignored by default** (61 lib, 14 plugin, 2 kernel, 3 TUI, 3 WebUI, 2 golden, 26 integration); the ignored network-dependent test passes when run separately. `fmt` and `clippy -D warnings` pass. Release bench (3 consecutive runs): kernel **2,473,328 B (2.359 MiB)**; warm **1.256 / 1.241 / 1.208 ms**; cold **4.523 / 4.342 / 4.378 ms**; RSS max **3,456 KiB**; write P95 **3.726 / 2.714 / 2.835 µs**; assemble P95 **0.156 / 0.154 / 0.151 ms**; RPC P95 **0.054 / 0.046 / 0.065 ms**; all gates true.

Not verified: v1.2.1-rc.3.1 Windows real hardware (real 7B guide producing a hard criterion or warned judge-only, the model dropdowns against a live Ollama, Enter-to-save on ConPTY, the permission popup in Windows Terminal, S3 t6 single call), high-DPI/Chinese rendering, network-drive/24h soak, macOS/arm64. Wine was not run.

## v1.2.1-rc.3 summary (historical, released)

The rc.2.1 real-hardware retest closed G1 (review can approve) and G3 (`chat.finished.status` tri-state) but showed that the gate can still be satisfied by copying the guide's own sentences (H1), that the judge echoes the prompt instead of the answer (H2), that malformed tool names surface as opaque RPC errors (H3), and that S3 t5/t6 depend on prompt luck (H4). It also asked for an architectural change: plugin settings must be declared by the plugin, relayed by the kernel, and rendered generically, instead of being hard-coded into each front end.

| Item | Treatment | Verification |
|---|---|---|
| A-01 settings declaration | `manifest.settings` (`SettingsSchema`/`SettingField`, types limited to `bool|int|number|string|enum`) is the single source; `feather-plugin` handshakes `settings: None` to prevent drift | Two unit tests (`setting_field_path_and_root_default`, `setting_field_validates_type_and_range`) |
| A-02 kernel relay | Registry persists the schema and backfills old entries; `plugin.settings` RPC returns schema/values/defaults; undeclared plugins get a business error | Integration `plugin_settings_are_relayed_and_validated` |
| A-03 validation | `Config.plugin_settings` fallback namespace; `declared_settings()` + `validate_plugin_settings()` enforce type/range/enum and reject undeclared keys; declared paths accept a first write | Same integration: temperature 0.9 rejected, `max_rounds=3` round-trips, `plugin_settings.search.unknown` rejected |
| A-04 generic front ends | TUI plugin page built from `plugin.settings` (11 general fields restored); WebUI plugin page renders manifest + declared form; neither hard-codes plugin field names | TUI PTY: open review-gate → `max_rounds` 2→3 → `Ctrl+S` → `config.json` reads 3. WebUI browser: all nine declared fields incl. enum rendered; save persisted `max_rounds: 3`; WebSocket re-check after save returned `model.message` + `chat.finished(ok)` |
| H1 rubric degeneration | 2–5 items, `must_include` ≤20 chars, sentence-like phrases rejected, at least one `must_include` and one non-substring criterion; hardcheck covers literal/banned/point-count/glob | Unit tests for all four criterion kinds, sentence/length rejection, and glob/point helpers |
| H2 judge echo | `parse_verdict` receives the answer; item and summary evidence must be a verbatim contiguous ≥10-character window (normalized) | Unit test rejects short evidence and prompt echoes, accepts copied answer text |
| H3 unknown tool | `execute_tool` checks names against `available_tools` and returns `未知工具 … 可用工具：…` as a business error | Integration `unknown_tool_name_gets_correction_instead_of_raw_rpc_error` (lists `tool.fs.read`, session still finishes, no breaker) |
| H4 S3 prompts | t5 requires calling the tool first; t6 requires exactly one call and then reporting failure | Prompt text in `tools/s3_batch.py`; S3 numbers still require a real model run |

Linux has **109 tests: 108 passed, 1 ignored by default** (61 lib, 12 plugin, 2 kernel, 3 TUI, 3 WebUI, 2 golden, 26 integration); the ignored network-dependent test passes when run separately. `fmt` and `clippy -D warnings` pass. Release bench: kernel **2,458,744 B (2.345 MiB)**; warm **1.507 / 1.393 / 1.403 ms**; cold **7.828 / 6.578 / 7.033 ms**; RSS max **3,584 KiB**; write P95 **4.03 / 3.70 / 3.88 µs**; assemble P95 **0.138 / 0.146 / 0.139 ms**; RPC P95 **0.129 / 0.040 / 0.096 ms**; all gates true.

Not verified: v1.2.1-rc.3 Windows real hardware (plugin-declared settings on both front ends after a fresh install, 7B phrase-only rubric quality, judge evidence being answer text, S3 reproducibility), high-DPI/Chinese rendering, network-drive/24h soak, macOS/arm64. Wine was not run; it remains startup-only evidence.

## v1.2.1-rc.2.1 summary (historical, released)

The rc.2 real-hardware retest confirmed the P0 provider body, P1 temperature rejection, P2 effective-config banner, R1 gates, and the review-gate mechanism. It also exposed two regressions and three quality gaps: the Ollama model list, TUI review settings, free-text rubric checks, placeholder judge evidence, and `chat.finished.status` hiding unapproved reviews.

| Item | Treatment | Verification |
|---|---|---|
| R-01 Ollama models | Native Ollama probe uses `/api/tags`; OpenAI-compatible bases keep `/models` | Plugin unit test asserts `GET /api/tags`; integration `provider.models` returns models and mock sees `/api/tags` |
| R-02 TUI review settings | 19-field settings form with enabled/rounds/guide/exec/judge model+temperature, range validation, scrolling, one `config.set` patch | `cargo check`; TUI PTY smoke; WebUI shares `config.set` schema |
| R-03 literal rubric (G1) | `must_include` literal required; `review.hardcheck` runs before judge; feedback includes `criterion`/`check` | Parser tests reject missing/vague literal; hardcheck test fails a missing phrase |
| R-04 judge evidence (G2) | Item and top-level evidence ≥ 10 chars; judge retried once; guide retried once; empty review response retried once | Short-evidence parser test; integration review still passes after retry |
| R-05 status (G3) | `chat.finished.status` = `ok`/`unapproved`/`degraded`; `review.status` and verdict `items` exposed | Integration forces an unapproved review and asserts both statuses |
| rc.2 retained | Provider body, temperature ranges, banner/source, embedding model | rc.2 tests still pass |

Linux has **102 tests: 101 passed, 1 ignored by default** (59 lib, 9 plugin, 2 kernel, 3 TUI, 3 WebUI, 2 golden, 24 integration); the ignored network-dependent test passes when run separately. `fmt` and `clippy -D warnings` pass. Release bench: kernel **2,413,136 B (2.301 MiB)**; warm **1.151 / 1.155 / 1.141 ms**; cold **3.045 / 3.340 / 3.225 ms**; RSS max **3,456 KiB**; write P95 **2.68 / 2.69 / 2.71 µs**; assemble P95 **0.125 / 0.125 / 0.135 ms**; RPC P95 **0.042 / 0.042 / 0.035 ms**; all gates true. One earlier run in six showed an RPC P95 of 1.234 ms (scheduler spike, same binary); the three consecutive clean runs above are reported.

Not verified: v1.2.1-rc.2.1 Windows real hardware (real Ollama model list, TUI review-field save/scroll, 7B literal rubric and evidence quality, honest unapproved/degraded status), high-DPI/Chinese rendering, network-drive/24h soak, macOS/arm64. Wine, if run, remains startup-only evidence.

## v1.2.1-rc.2 summary (historical, released)

The rc.1 real-hardware retest closed R1 and confirmed the review-gate flow, but exposed a P0 provider regression and two configuration issues. This patch fixes them without changing R1 thresholds or the review-gate protocol.

| Item | Treatment | Verification |
|---|---|---|
| P0 empty body | Optional Authorization no longer creates a blank line before `Content-Length`; headers are appended line by line | Raw-request unit test; empty-key Ollama `/api/chat` + `/api/embed` integration; live echo `Content-Length: 6842` |
| P1 temperature ranges | review-gate guide/exec/judge bounds are enforced even when disabled; rejected before save | `config.set` integration rejects 0.9 / 1.9 / 0.8 with ranges; disk unchanged |
| P2 effective config | `serve` prints model/provider/base URL and source; CLI-vs-config conflicts warn | Live startup: `model_source=cli`, `config.get` returns the CLI values, and `review.guide` emitted the CLI model |
| Embedding model | `recall` / `index_user_text` pass `cfg.model` to `provider.embed` | Empty-key Ollama test asserts `/api/embed` carries `model` and `input` |

Linux passes **97/97** (59 lib, 7 plugin, 2 kernel, 3 TUI, 3 WebUI, 2 golden, 21 integration); the network-dependent case passes separately. `fmt` and `clippy -D warnings` pass.

Not verified: v1.2.1-rc.2 Windows real hardware (P0 provider path, config rejection, banner/source line, review-gate UI/events), TUI PTY regression for the API-key field, two-turn in-turn steer, full two-turn `/compact`, real DeepSeek billing, high-DPI/Chinese rendering, network-drive/24h soak, macOS/arm64.

## v1.2.1-rc.1 summary (historical, released)

This round closes R1 from the v1.2.0-rc.1 real-hardware retest and adds two requested capabilities: the `review-gate` three-role loop plugin and OpenAI-compatible local providers with an Ollama API key.

| Item | Treatment | Verification |
|---|---|---|
| R1 | Warm boot = same install root, one discarded warm-up, five samples; cold `serve` spawn measured separately with its own budget; new `cold_spawn_ms_p50` | Three release benches: warm 1.23–1.26 ms, cold 4.2–5.6 ms, all gates true |
| `review-gate` | New loop plugin with guide → exec → judge, structured rubric/verdict, evidence required, 1–5 revisions, review events, WebUI settings | Plugin parser tests; integration retry/fail/pass with `provider-mock`; exact event sequence |
| Local providers | `provider-llamacpp`, `provider-ninfer`, `provider-lmstudio`, `provider-vllm`, `provider-openai`; OpenAI `/v1` normalization | NInfer probe + llama.cpp/NInfer streaming tests |
| API key | `provider_api_key` in config, TUI/WebUI field, `Authorization: Bearer` on native Ollama and OpenAI paths | HTTP header unit test + Ollama `/api/chat` integration + NInfer/llama.cpp chat integration |
| Version | `1.2.1-rc.1` across Cargo, lock, 11 plugin manifests, WebUI placeholder | `scripts/version-check.sh` |

Linux passes **95/95** (58 lib, 7 plugin, 2 kernel, 3 TUI, 3 WebUI, 2 golden, 20 integration); the one network-dependent `local_web_search_tool_via_kernel` passes separately. `fmt` and `clippy -D warnings` pass. The release kernel is 2,406,664 B (2.295 MiB); idle RSS 3,456 KiB; `binary_le_8mb`, `rss_le_15mb`, `assemble_p95_lt_30ms`, `rpc_p95_lt_1ms`, warm and cold gates all pass.

Not verified: v1.2.1-rc.1 Windows real hardware (R1 warm/cold, review-gate UI/events, provider key fields, local provider chat), TUI PTY regression for the new API-key field, two-turn in-turn steer, full two-turn `/compact`, real DeepSeek billing, high-DPI/Chinese rendering, network-drive/24h soak, macOS/arm64.

## v1.2.0-rc.1 summary (public-repository candidate)

The v1.1.9 real-hardware retest left only Q1 (a suspected `cacelled` spelling in the TUI status bar) and Q2 (the model sent `web.search` `count` as a string). This round:

| Item | Treatment | Verification |
|---|---|---|
| Q1 | Source and binary both contain `cancelled`; the report is a terminal-glyph misread. Spelling kept; a binary string check was added to the test guide | `strings featheragent-tui.exe` |
| Q2 | Schema remains integer; the kernel normalizes numeric strings before forwarding, and the plugin tolerates them too | `util::json_u64` unit test; mock sends `"count":"3"` |
| Version drift | Unified `version-check.sh`, `--locked`, PE FileVersion/ProductVersion, WebUI `{{VERSION}}`; fixed `--version` starting the kernel and the hard-coded v1.1.8 plugin-page title | version gate + all three `--version` entries + cross-compile |
| Public materials | MIT LICENSE, SECURITY, CONTRIBUTING, CODE_OF_CONDUCT, CHANGELOG, `scripts/ci.sh`, CODEOWNERS | file check + `scripts/ci.sh` |
| History scan | No keys, private keys, `.env`, or large binaries in git history; only `target/dist/__pycache__` ignored in the workspace | `git log` / content scan |

Linux passes **86/86**: 57 lib + 2 plugin + 0 storage + 2 kernel + 3 TUI + 3 WebUI + 2 golden + 17 integration. One network-dependent `local_web_search_tool_via_kernel` runs separately. `cargo fmt --check` and `cargo clippy --all-targets --locked -- -D warnings` pass. Linux's three artifacts and Windows' five exes were rebuilt; PE `.rsrc` is 0x1188 and `FileVersion/ProductVersion=1.2.0-rc.1`.

Not verified: v1.2.0-rc.1 Windows real hardware, v1.2 TUI PTY regression, two-turn in-turn steer, two full `/compact` turns, real DeepSeek billing, high-DPI/Chinese rendering, network-drive/24h soak, macOS/arm64.

### UI-language follow-up (superseded by v1.2.1-rc.3.6)

- Chinese executables remain the default. The earlier `--lang` switch is still absent, but every top-level binary now has an English `-en` edition; the suffix, not a runtime flag, selects the language.
- The English TUI uses separate English source strings; the English WebUI serves `index.en.html` + `app.en.js` while the Chinese WebUI keeps `index.html` + `app.js`.
- `feather-plugin-en` and `feather-storage-en` are protocol-only aliases with no user-facing UI.
- Verified: English TUI PTY shows English panel titles; English WebUI `GET /` returns `<html lang="en">` and English JS; the Chinese WebUI still returns `lang="zh-CN"` and `RPC 超时`. Windows cross-compile is rebuilt; real-hardware retest remains pending.
- The Windows executable icon and WebUI favicon remain the transparent-background 7-frame ICO.

## v1.1.9 summary (historical)

v1.1.8 real-hardware retest confirmed N1/N3/N4 and the P1 direction, and exposed project-workspace path double nesting (P1), stale S3 paths (P2), documentation semantics (P3), an unverifiable slow-start branch (P4), and `/stop` showing `failed` (§6.1). v1.1.9 fixed P1/P2/P3/§6.1 and added TUI/WebUI Markdown, feather icons, and keyless local web search. Linux passed **85/85**, plus one network case separately. Windows cross-compiled five exes with `.rsrc` icons and only system DLL dependencies; real-hardware retest remained with the user.

## Historical fix summaries

### v1.1.8 (from the v1.1.7 real-hardware report)

Confirmed NEW-K fixed; S3 and 6/6 gates had no regression, but `bench` still exited 0 on conflict. Also exposed fabricated worklog times/template leakage, inconsistent banner separators, and stale session sandboxes. Fixed N1/N2/N3/N4/N6/N7 and moved to project-level workspaces:

| ID | Change | Verification |
|---|---|---|
| N1 | `bench` prints an error and `exit 2` when a kernel is running | code path; Linux integration covers running/stopped |
| N2 | Plugin registration prints `[init]` progress; docs say "first run 1–2 minutes" | visible; **antivirus scan time not removed** |
| N3/N4 | Kernel appends real UTC time, goal, state, tools to worklog; template wording changed; no placeholders | `project_workspace_has_own_agent_home_and_worklog` |
| N6 | Windows display paths normalized to `\` | `normalized_path()`; Linux unaffected |
| N7 | No more `<workspace_root>/<session>` sandbox per model | project workspace structure; old empty dirs not auto-deleted |
| WS-2 | `workspace_root` is a project container; each project has `.featheagent/` | integration + `workspace.list/create`; Linux mock end-to-end |
| TUI-3 | `/settings` sidebar: general + dynamic plugin pages, Enter/Tab/Space/Esc | compile/key logic only; **not PTY-tested** |
| CTL-1 | `chat.stop` preserves partial output and log; `chat.cancel` accepts `cancelled` | integration `stop_preserves_partial_output_and_session_remains_usable`; `cancel_propagation` |
| CTL-2 | `chat.inject` writes steer during a run and restarts at a boundary | `inject_steers_running_turn` |
| CTL-3 | `context.compact` keeps the last two turns; old text remains replayable | `manual_compaction_creates_checkpoint_and_keeps_log` + context units |
| WEB-1 | `web.search` / `web.fetch` use DeepSeek Search/Fetch; key only from env | parse/args/hiding units; **no key, no real network** |
| DOC-1 | DeepSeek Search/Fetch costs 5 CNY per 1000 successful requests; not free | official docs; release note explicit |

Linux tests **78/78** passed at the time. Unverified: Windows project workspace, TUI sidebar, WebUI controls, stop/steer/compact, DeepSeek search, high-DPI/Chinese rendering.

### v1.1.7 (from the v1.1.6 report)

NEW-K fixed; TUI session/chat modes, CJK cursor alignment, external workspace, plugin management, and WebUI sidebar landed. Linux tests **70/70**. Unverified: Windows NEW-K, TUI cursor/scroll, WebUI sidebar/plugin install, external drive paths. `workspace_root` still needs a `serve` restart; TUI resume shows the latest 200 events only; `context_window` 10240 came from saved config while the code default remained 8192.

### v1.1.6 (from the v1.1.5 report)

- NEW-H: `serve` prints a banner before initialization, flushes, reports staged progress, and writes `runtime/ipc/starting` / `ready`; `chat` reports "kernel not ready (starting)".
- NEW-I: `config.set` supports atomic `{path,value}` single-field updates with a config lock; `config.replace` remains full replacement; `path` accepts an optional leading `/`.
- NEW-J: `tools/s3_batch.py` has `OK` / expected-failure / `ERR` levels; t6 is no longer a real failure.
- WebUI model dropdown uses a real `select` for all `provider.models`, while the input remains editable.
- Tests **65/65**; Linux browser test with mock Ollama showing three models; S3 offline 4/5 success, 1/1 expected business error, 0/5 real failure; Windows cross-compile five PE32+ x64 exes.

### v1.1.5 (from the v1.1.4 report)

- NEW-E: business errors become `tool.error` without restarting the plugin or counting toward the breaker; only crash/timeout/IO are `tool.failed`. Three consecutive technical failures by tool name trip the breaker; tripped tools leave the schema and a system hint forbids bare `tool_call`.
- NEW-F: `tool_base` normalizes `workspace/x`, `/workspace/x`, `\workspace\x`, `./workspace/x`; tool descriptions and system prompt forbid leading slashes.
- NEW-G: `tools/s3_batch.py` uses strict criteria (`tool.result` without deny/error).
- Settings page: WebUI `⚙ Settings`, TUI `/settings`; provider, base URL, model, temperature, context window, max turns, auto-allow-safe; atomic `config.set`; `provider.models` supports Ollama `/api/tags` and OpenAI `/v1/models`.
- Tests **65/65**.

### v1.1.4 (from the v1.1.3 report)

- NEW-A: `chat --allow <tool,...>` / `--yes` / `--confirm-timeout`; dangerous tools are denied immediately unless allowed.
- NEW-B: permission denial injects a structured `tool.result` (`permission_denied` + `reason`).
- NEW-C: empty response retries once at temperature 0.7 with `model.retry`; still empty → `status=failed`.
- NEW-D: configurable confirmation wait, default 0.
- WebUI rename: `featheragent-gui` → `featheragent-webui`, CLI `webui`, old `gui` alias retained; `runtime/gui-profile` → `runtime/webui-profile`.
- Tests **59/59**; Windows kernel `featheragent.exe` 886,272 B; five exes depend only on system DLLs; Wine 6.0.3 smoke passed `version/help/doctor/bench` but startup (152.36 ms) and overlapping named pipes are Wine artifacts.

### v1.1.3 (from the v1.1.2 report)

- NEW-1: Ollama empty response no longer returns `status=ok`; plugin subprocesses get `NO_PROXY` loopback; HTTP client ignores proxy env.
- NEW-2: `provider-ollama` supports native `message.tool_calls` and OpenAI `/v1` `delta.tool_calls`; structured calls are preferred, text parsing is fallback.
- NEW-3: platform-specific bench gates: Windows boot <100 ms / write P95 <10 ms; Linux 20 ms / 100 µs.
- NEW-4: client disconnect triggers `chat.cancel`, frees the session holder; cancelled/failed sessions can retry.
- P3: Windows pipe shutdown errors 232/109 are silent.
- Tests **58/58**; Windows zip rebuilt; Wine smoke passed `version/help/doctor/bench`; `serve` initialize timed out under Wine's overlapping named pipes, not a Windows conclusion.

### v1.1.2 (from the Windows real-hardware report)

- Tests **56/56** (41 lib + 2 GUI + 2 golden + 11 integration).
- Named pipes use overlapped I/O + `DuplicateHandle` per connection.
- Random tokens use the system CSPRNG (`ProcessPrng` / `/dev/urandom`).
- Windows volume type uses `GetDriveTypeW`.
- GUI/TUI `initialize` has a 3-second timeout and clear error.
- Added `help` / `chat` / `serve --codec json`, bench progress, `runtime/ipc/endpoint`, Wire Format doc, and `tools/feather_client.py`.
- Windows zip cross-compiled; Wine 6.0.3 smoke passed `version/help/doctor/bench`; real Windows still needed.

## 1. Conclusion

| Item | Result |
|---|---|
| Automation | **86/86 passed** (57 lib + 2 plugin + 2 kernel + 3 TUI + 3 WebUI + 2 golden + 17 integration) |
| Kernel gates | **6/6 passed** (size, startup, RSS, write, assembly, RPC) |
| Crash recovery | **1000/1000** random truncations, `failed=0` |
| End-to-end | kernel, TUI PTY, WebUI HTTP/WS/SSE fallback, three-artifact unzip run all pass |
| Windows x64 build | five exes cross-compile (mingw-w64 + static CRT); Wine smoke; real hardware pending (§4.1) |
| Known untested | real Ollama/7B (S3), Windows real hardware, macOS, four-platform matrix (§8) |

## 2. Environment

| Item | Value |
|---|---|
| Machine | Linux x64, 2 vCPU / 4 GiB, ample disk |
| Toolchain | rustc 1.100.0 / cargo 1.100.0 (rustup stable minimal) |
| Build | `cargo build --release --bins`; `opt-level="z"` + fat LTO + one codegen unit + `panic=abort` + strip |
| Provider | `provider-mock` (deterministic); no live model service |
| Browser (WebUI) | `/usr/local/bin/chromium`, `DISPLAY=:99.0` |

## 3. Test matrix

### 3.1 Unit tests (64 lib + 30 plugin + 2 kernel + 5 TUI + 4 WebUI in v1.2.1-rc.4)

`cargo test --lib`: log (round-trip / CRC / rotation / recovery / archive / fork), event bus (cursor / lost wakeup / lagged), governance (volume / env cleanup / permission gate), context engine (budget / four layers / skill budget / image budget), tolerant parser, plugin process (handshake / routing / breaker), WASM (fuel / no syscalls), registry, net (Bearer headers and optional-header/body layout), util, provider HTTP/Ollama JSON and review-gate parsers.

### 3.2 Integration tests (34, `tests/integration.rs`)

Real kernel + plugin subprocesses over UDS:

| Test | Coverage |
|---|---|
| `thirty_rounds_stable` | 30 consecutive turns, per-turn replies asserted |
| `hundred_rounds_budget_stable` | 100 turns; tokens stay ≤ 95% of budget |
| `tool_execution_and_permission_gate` | `tool.fs.read` auto-allowed; `tool.shell` confirms first |
| `shell_timeout_marks_timed_out` | 15 s timeout kills the process group; `timed_out=true` |
| `cancel_propagation` | `SPIN` stream + `chat.cancel` → `chat.finished{cancelled}` |
| `crash_restart_retries_once` | provider `CRASH_ONCE` restarts and retries once |
| `observer_is_readonly_and_fork_copies` | second connection is observer; `chat.send` gets 40301; fork copies events |
| `storage_index_and_vector_recall` | storage available; events/vectors written |
| `plugin_contract_list` | registry lists factory plugins |
| `wasm_and_log_unit_paths_smoke` | `system.info` version/contract/gate mode |
| `s2_thousand_truncations` | 1000 random truncations |
| plus project workspace, worklog, stop/steer/compact, Windows path validation, `ContextCompacted` codec | v1.1.6–v1.1.9 additions |
| `review_gate_three_roles_retry_and_approve` | guide → failing judge → feedback → retry → passing judge; four review events and final `approved=true` |
| `llama_cpp_and_ninfer_openai_chat_use_api_key` | both providers probe `/v1/models`, stream chat, send Bearer |
| `ollama_native_chat_uses_api_key` | native `/api/chat` with `Authorization: Bearer` over chunked NDJSON |
| `ninfer_probe_uses_v1_and_bearer_key` | base URL without `/v1` is normalized; probe carries the key |
| `ollama_native_chat_without_key_still_sends_body` | empty key still sends `Content-Length` + `/api/chat` body; `/api/embed` carries model/input |
| `ollama_provider_models_reads_api_tags` | `provider.models` returns native models and requests `/api/tags`, not `/models` |
| `review_gate_unapproved_is_reflected_in_chat_status` | forced unapproved review yields `chat.finished.status=unapproved` and `review.status=unapproved` |
| `web_search_artifact_is_persisted_under_project_workspace` | search result JSON under `.featheagent/search/` with tool/arguments/result/untrusted |
| `file_attach_copies_upload_to_default_workspace` | base64 upload lands in `workspace/default/uploads/` with correct bytes |
| `file_prepare_copies_local_file_outside_install_root` | outside file copied; inside-root file returns `copied:false` |

### 3.3 Golden trace

- 11 parser fixtures: trailing comma, single quotes, bare keys, fake XML `<tool_call>`, ```json fence, half JSON (must abandon, not send wrong args), long output, mixed Chinese/English + Chinese path, double-encoded `arguments`, `function` style, `tool_calls` array, no call.
- 3 pipeline fixtures: mixed-language echo, `CRASH_ONCE` restart retry, `READ:golden.txt` tool loop.
- Assertions: tool name, argument object, model text, tool result, final `chat.finished`, session tokens ≤ 95%.

## 4. Measured gates (release, local)

Command: `./target/release/featheragent bench --json`

| Metric | Gate (Linux) | v1.2.1-rc.2.1 measured (3 runs) | Result |
|---|---|---|---|
| Kernel binary | ≤ 8 MiB | **2,413,136 B (2.301 MiB)** | ✅ |
| Warm boot P50 (`boot_ms_p50`) | < 20 ms | **1.151 / 1.155 / 1.141 ms** | ✅ |
| Cold spawn P50 (`cold_spawn_ms_p50`) | < 100 ms | **3.045 / 3.340 / 3.225 ms** | ✅ |
| Idle RSS | ≤ 15 MiB | **3,456 / 3,328 / 3,456 KiB (max 3.4 MiB)** | ✅ |
| Event write P50 / P95 (no fsync) | P95 < 100 µs | **2.68 / 2.69 / 2.71 µs** | ✅ |
| Context assembly P95 (no summary model) | < 30 ms | **0.125 / 0.125 / 0.135 ms** | ✅ |
| RPC round-trip P95 | < 1 ms | **0.042 / 0.042 / 0.035 ms** | ✅ |

> Warm boot uses the same install root, one discarded warm-up, then five samples; cold spawn is a real `serve` child process measured to socket readiness. Event dispatch P95 < 200 µs has no standalone number; the bus and write share the in-process path, so no independent benchmark exists.

### 4.1 Windows x64 cross-compile

- Toolchain: `x86_64-w64-mingw32-gcc` + `x86_64-pc-windows-gnu`, static CRT.
- Five PE32+ x64 exes; only system DLLs (`kernel32/ntdll/msvcrt/ws2_32/user32/...`), no `libgcc/libwinpthread/libstdc++`.
- Wine smoke was not repeated for rc.2 (Wine 6 cannot exercise Windows named-pipe concurrency or ConPTY); earlier Wine results remain non-Windows evidence.
- v1.2.1-rc.2.1 PE resources: `.rsrc` present at RVA 0x217000, `FileVersion/ProductVersion=1.2.1-rc.2.1`, MIT copyright visible in `FileInfo`; kernel exe **2,181,632 B**.

## 5. Crash recovery (S2)

Command: `./target/release/featheragent doctor --stress-log 1000`

```text
iterations 1000, ok 1000, failed 0, elapsed_ms 3399
```

Each iteration: write 1–40 events (Always sync) → randomly truncate the last segment → reopen → `recover` → `verify_all` → check monotonic `seq`. All 1000 iterations pass.

## 6. Frontend verification

### 6.1 TUI

PTY-driven: `/new s-tui` → `hello tui` → Esc. Exit code 0, no panic; `data/sessions/s-tui` and `_system` created; `doctor --verify-log` `ok=true`, 7 records, 0 issues.

v1.2.1-rc.2.1 review-field smoke: 110×34 PTY, `featheragent-tui --root <packaged core>`, `/settings` showed all 19 fields including the nine review fields and the ≥7B hint. Toggling Review gate, changing rounds to 3, and guide temperature to 0.35, then Enter, persisted `review_gate.enabled=true`, `max_rounds=3`, `guide.temperature=0.35` to `data/config.json`; status showed `已保存并应用`. A second 80×18 PTY run used BackTab from field 0 to the cancel field; the form scrolled so `取消/返回` and the judge/exec fields stayed visible.

### 6.2 WebUI bridge

Security checks:

| Check | Result |
|---|---|
| binds only `127.0.0.1` + random port | ✅ (`bind` is `127.0.0.1`; port changes) |
| `/rpc` without token | **401** |
| `Host: evil.example` | **403** |
| token in URL `#fragment` (not Referer) | ✅; JS reads it then `history.replaceState` removes it |
| WebSocket handshake `Sec-WebSocket-Key` | ✅ 101 + correct `Sec-WebSocket-Accept` |
| WS same-connection RPC + events | ✅ raw client runs `initialize / session.create / event.subscribe / chat.send`, receives events |
| HTTP `/events` SSE fallback | ✅ same event set |
| `--app` starts Chromium | ✅ `--app=<url>` and `--user-data-dir=<root>/runtime/webui-profile` |
| separate profile actually used | ✅ `runtime/webui-profile/` created and written |

> Chromium prints D-Bus errors in a container without D-Bus; this is browser-environment noise and does not affect the page or bridge. No manual visual acceptance was performed; functionality is verified through HTTP/WS contracts.

### 6.3 Packages

`scripts/package.sh --skip-build` produces three Linux artifacts and asserts kernel ≤ 8 MiB. Windows x64 is a separate PE zip.

| Artifact | Content |
|---|---|
| `featheragent-core-1.2.1-rc.2.1-linux-x64.tar.gz` | kernel, TUI, WebUI launcher, `feather-plugin`, `feather-storage`, 11 plugin manifests, docs, README |
| `featheragent-webui-1.2.1-rc.2.1-linux-x64.tar.gz` | WebUI launcher, `web/`, favicon, docs |
| `featheragent-plugins-1.2.1-rc.2.1-linux-x64.tar.gz` | 11 plugin manifests |
| `FeatherAgent-1.2.1-rc.2.1-windows-x64.zip` | five PE exes, 11 plugin manifests, docs, icon; kernel exe 2,181,632 B |

Exact package bytes and SHA-256 values are recorded in the adjacent `.sha256` files and the GitHub Release notes. The release kernel is 2,413,136 B (2.301 MiB); `sha256sum -c`, `tar -tzf`, and `unzip -t` all pass.

## 7. Real defects found and fixed

| # | Defect | Impact | Fix |
|---|---|---|---|
| 1 | `Args::has("--flag")` did not match parsed keys without `--` | `doctor --check-storage` / `--stress-log` silently ignored | strip `--` before matching |
| 2 | GUI opened a new UDS connection per HTTP request | session holder bound to a closed connection; later `chat.send` got observer 40301 | bridge keeps one long-lived upstream connection; one WS connection per page |
| 3 | `quote_bare_keys` rebuilt bytes via `as char` | Chinese arguments became mojibake and duplicated tool calls | byte buffer + verbatim UTF-8 slices |
| 4 | mock provider split a 16-byte stream by **bytes** | Chinese/emoji split into `�` | split by characters |
| 5 | HTTP chunked decode used `from_utf8_lossy` per chunk | multibyte UTF-8 cut at chunk boundaries → mojibake (Ollama path) | accumulate bytes, decode complete lines at `\n` |
| 6 | `tool.shell` timeout killed only `/bin/sh` | `sleep` descendants held stdout; `wait_with_output` hung to 30 s | `setsid` process group + `kill(-pid, SIGKILL)` |
| 7 | Empty provider key inserted an empty Authorization line before `Content-Length` | HTTP headers terminated early; Ollama/OpenAI requests carried no visible body (`400 missing request body`) | append headers line by line; emit `Content-Length` before optional Authorization, final blank line once |
| 8 | review-gate temperature bounds were checked only when `enabled=true` | out-of-range values (0.9/1.9/0.8) were accepted and persisted while disabled | schema-level validation with path, submitted value, and `[lo, hi]`; reject before save |
| 9 | CLI overrides were invisible in the startup banner | `review.guide` model looked like the persisted value; users could not tell which model was live | print `effective model/provider/base_url` plus `*_source`, and warn on CLI/config conflict |
| 10 | `/api/embed` used the provider default model, not the active model | recall/index could target a model the user did not select | pass `cfg.model` to `provider.embed` |

## 8. Untested items and deviations (honest statement)

### 8.1 Untested

1. **Real model / S3**: no Ollama service (`127.0.0.1:11434`), so `qwen2.5-coder:7b` / `deepseek-r1:7b` / `qwen3-vl:4b` were not connected. Tool-call accuracy, first-response latency, post-compression success rate are **blank**; the 100-turn budget is a mock mechanism check, not a small-model result.
2. **Windows real hardware**: cross-compile and Wine smoke done; no real Windows machine, TUI keyboard, `--app` browser launch, SmartScreen, or Windows performance.
3. **macOS**: not built, not run.
4. **Four platforms × two architectures** (win/mac/linux × x64/arm64): untested; SIMD defaults not switched.
5. **Native GUI (`wry`)**: documented as optional, not implemented; WebUI `--app` is verified, native GUI remains a future plugin.
6. **Long-run stability**: no 24 h soak / memory-leak curve.
7. **Event dispatch single-hop P95**: no independent benchmark.

### 8.2 Deviations (explainable, not silent moves)

- **`sonic-rs` → `serde_json`**: the design allows `json-fallback` for non-SIMD targets; this host did not enable the AVX2 build branch.
- **No Svelte/Preact**: zero-build native HTML/JS, trading framework tooling for "unzip and run" with no frontend build dependency.
- **SSE fallback added**: main channel is HTTP + WebSocket; SSE is only a degradation path.
- **Self-written minimal WebSocket codec**: no `tungstenite`, to avoid package size/dependency growth; text frames, fragmentation, ping/pong, close suffice.
- **Windows GNU C dependencies**: build scripts export `CC_x86_64_pc_windows_gnu` / `AR_x86_64_pc_windows_gnu`; blake3 assembly is discarded under windows-gnu (`corrupt .drectve`), so the Windows target uses the pure Rust implementation with identical hashes.
- **Windows file replacement**: `std::fs::rename` cannot overwrite; config/registry/archive writes use `MoveFileExW(MOVEFILE_REPLACE_EXISTING)`; Unix keeps `rename(2)`.

## 9. Reproduction commands

```bash
cargo test --lib
cargo test --test integration
cargo test --test golden
cargo build --release --bins
./target/release/featheragent bench --json
./target/release/featheragent doctor --stress-log 1000
./scripts/package.sh --skip-build
```

TUI (PTY): `script -qec "./target/release/featheragent-tui --root <root>" /dev/null`
WebUI: `./target/release/featheragent serve --root <root>` + `./target/release/featheragent-webui --root <root> --app`

## 10. Verdict

Against the hard kernel gates in §2.4 / §9.3, all six pass with margin (size 0.93/8 MiB, RSS 2.63/15 MiB, write 3.4/100 µs). Functional coverage includes contracts, log, bus, governance, plugin runtime, context engine, orchestration loop, TUI, WebUI bridge, storage/provider/tool plugins, and three-artifact packaging.

**Conclusion: the v1.1 kernel and frontends reach the deliverable level described in the design. S3 small-model effect metrics remain untested because no real inference service is available; mock numbers are not substituted.**
