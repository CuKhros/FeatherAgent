# Changelog

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow semantic versioning. `v1.2.1-rc.4.9` is the current public release candidate and does not replace a stable release.

## [1.2.1-rc.4.9] - 2026-11-27

### Fixed
- N43: empty judge evidence is no longer grouped with “evidence too short”. `parse_verdict` checks the empty string first and returns “invalid vote”; the engine raises only that vote's budget from two to three attempts and the retry note says it is not a failed criterion. Tests: `verdict_empty_evidence_is_invalid_vote_not_short_evidence`, `review_gate_empty_evidence_retries_and_approves`.
- N44: when the body is already generated and only the trailing pseudo-XML marker is malformed, the provider keeps the body, emits `review.warn`, and no longer sends `error` that fails the round. `done` carries the full `text` and optional `warn`; `strip_malformed_tool_markup` only cuts the tail. Tests: `malformed_xml_tail_is_stripped_after_content_is_kept`, `provider_malformed_xml_tail_keeps_answer_and_warns`.
- N45: the math/proof prompt now says: keep a sound derivation; if you choose to verify, call the tool and wait; never negate an existing derivation without a tool result. Test: `system_prompt_separates_content_output_from_file_creation`.

### Added
- `docs/model-test-reports/reports/Windows真机复测报告-v1.2.1-rc.4.8.md` and `data/math-papers-rc.4.8-targeted.csv`; the report index is now 15 reports.

### Changed
- `review.warn` payload adds `source` / `kind` / `model` / `retry`; `parse_verdict` distinguishes “evidence 为空” from “evidence 过短”.
- N46 gets prompt pacing only; no hard length gate.

### Documentation
- Linux suite: 217 items, 216 passed, 1 ignored (87 lib, 34 plugin ×2, 5 kernel, 6 TUI, 4 WebUI ×2, 2 golden, 41 integration with 40 passed and 1 ignored); `scripts/ci.sh` passed.
- Linux release bench: kernel 2,581,680 B; warm P50 1.383/1.366/1.384 ms; cold P50 6.497/6.355/7.418 ms; all eight boolean gates true; reliability high on all three.
- Windows x64: 10 exes, kernel 2,320,896 B, system DLLs only. Wine smoke passed `version`/`doctor`/three bench runs/serve/chat and one N42 reconnect (`run=3`); 6 of 8 Wine gates passed, with `boot_lt_100ms` and `cold_spawn_lt_250ms` red; Wine is not a Windows baseline.
- Added bilingual rc.4.9 change notes; README, Windows guide, verification report and wire-format version now target rc.4.9.

## [1.2.1-rc.4.8] - 2026-11-27

### Fixed
- N40: ML02 formula checks now accept `π²/6`, `π^2/6`, `π^{2}/6`, `pi^2/6`, `(π^2)/6`, `(π²)/6`, `\frac{\pi^2}{6}`, `\dfrac{\pi^2}{6}`, `\frac{\pi^{2}}{6}`, `\frac{1}{6}\pi^2`, `\zeta(2)` and `ζ(2)`; ML10 also accepts `\frac{\pi}{16}` and `\dfrac{\pi}{16}`. Regression test: `math_paper_formula_variants_and_steps_are_scored`. This is still regex coverage, not numeric equivalence.
- N41: ML10 is split into four one-point checks: substitution method, substitution expansion, reduction/double angle, and the `π/16` result. Both trig-substitution and Beta routes can earn their steps.
- N42: stale turns no longer leak into a new client. `Session.run_id` gives each `chat.send` a generation; `run_turn` exits silently when superseded; `cancel_connection` invalidates the old run and marks the session `Cancelled`; `session.create` cleans an old holder; the `chat` CLI only accepts `chat.finished` with its own `run`. Reproduced 1/5 on rc.4.7, 5/5 clean on rc.4.8. Test: `killed_client_does_not_leak_or_block_next_run`.

### Added
- `docs/model-test-reports/reports/Windows真机复测报告-v1.2.1-rc.4.7.md` and `data/math-papers-rc.4.7.csv` (28 per-question scores); the report index is now 14 reports.

### Changed
- `chat.send` result adds `run`; `chat.finished.payload` adds `run`. Both are additive; old clients can ignore them, but clients that do not filter by run can still see stale events.
- `session.create` now cancels an old same-session holder owned by another connection before taking over.

### Documentation
- Linux suite: 211 items, 210 passed, 1 ignored (83 lib, 34 plugin ×2, 5 kernel, 6 TUI, 4 WebUI ×2, 2 golden, 38 integration).
- Linux release bench: kernel 2,581,016 B; warm P50 1.408/1.356/1.338 ms; cold P50 5.786/5.432/5.320 ms; all eight boolean gates true; reliability high on all three.
- Windows x64: 10 exes, kernel 2,320,896 B, system DLLs only. Wine smoke passes `version`/`doctor`/bench/serve/chat and one N42 reconnect; Wine is not a Windows baseline.
- Added bilingual rc.4.8 change notes; README, Windows guide, verification report and wire-format version now target rc.4.8.

## [1.2.1-rc.4.7] - 2026-11-26

### Fixed
- N36: judge evidence now uses a Chinese/English-aware floor: at least 6 CJK characters, 3 English words, or 10 ASCII alphanumeric characters. The 9-character `三条要点已写入文件` quote is accepted; placeholders and prompt echoes are still rejected. Tests: `verdict_accepts_short_chinese_evidence`, `verdict_accepts_three_word_english_evidence`.
- N37: a `permission_denied` `tool.result` carries a `hint` telling the model not to retry the same tool and to finish in prose; the system prompt repeats that rule and requires full math/proof derivations.
- N38: the system prompt requires math/proof answers to cover givens, each transformation and the conclusion; title-only or final-answer-only stops are explicitly discouraged. This is prompt mitigation, not an automatic continuation gate.
- N39: documented formula-spelling sensitivity in the math-paper scoring rules; ML02 accepts `π^2/6`, `pi^2/6` and `\frac{\pi^2}{6}`.

### Added
- Public exam-papers library adds `math-small` (8 questions / 25 points) and `math-large` (20 / 56), now 7 papers / 90 questions / 252 points.
- `docs/model-test-reports/reports/Windows真机复测报告-v1.2.1-rc.4.6.md` and `data/math-papers-rc.4.6.csv`; the report index is now 13 reports.

### Changed
- `tool.result` for denied tools adds `hint`; `review.verdict` fields are unchanged.
- English and Chinese change notes for rc.4.7; README status and Windows guide now target rc.4.7.

### Documentation
- Linux suite: 208 items, 207 passed, 1 ignored (83 lib, 34 plugin ×2, 5 kernel, 6 TUI, 4 WebUI ×2, 2 golden, 36 integration).
- N37 still requires `tool.shell` to be allowed when a math run should use sympy; N38 and N36 still need a Windows real-model retest.

## [1.2.1-rc.4.6] - 2026-11-26

### Fixed
- N32: judge output no longer dies on Windows path backslashes. `src/review.rs` parses standard JSON first, then repairs only invalid escapes / raw control characters inside strings; `json_slice` accepts objects and arrays. Evidence paths sent to the judge use `/`, and the judge prompt requires `/` and `\\`. Tests: `verdict_repairs_windows_backslash_escapes`, `rubric_repairs_windows_backslash_escapes`, `json_slice_accepts_top_level_array`.
- N33: `Config.max_output` (default 2048, range 128–8192) replaces the two hard-coded 512 `ModelProfile` caps; `run_model` and review calls pass it to the provider; Ollama gets `options.num_predict`, OpenAI-compatible gets `max_tokens`. `ModelMessage` exposes `truncated`, and a review role ending with `finish=length` fails and retries. Tests: `truncated_review_output_is_an_error`, provider request assertions.
- N34: `bench --json` `gates` now contains only boolean verdicts; `diagnostics` holds platform, raw readings, status, thresholds and reliability; `advisory_gates` holds advisory items. CI rejects non-boolean `gates`. Consumers must read `write_gate_status` / `write_gate_raw` from `diagnostics`.
- N35: `env_fingerprint` adds `cpu_load_pct` from Linux `loadavg_1m/cpu_count` or Windows `GetSystemTimes`; `merge_reliability` takes the worse of memory and CPU (≥70 medium, ≥85 low). `reliability_basis` reports both dimensions. Tests: `reliability_uses_worse_of_memory_and_cpu`, `env_fingerprint_reports_cpu_and_memory_basis`.

### Changed
- Extracted `src/review.rs` (924 lines) from `src/bin/plugin.rs`; `plugin.rs` drops from 3,028 to 2,219 lines. `parse_rubric_with_input` keeps one early `file_task` branch and removes the unreachable duplicate.
- Rebuilt the Chinese/English Rust UI generation chain from the rc.3.6 166/72 baseline; `tui_en.rs` and `kernel_en.rs` are regenerated, have no Chinese residue, and regeneration is deterministic.
- `gen-en-ui.py` generates Rust UI only. `web/app.en.js` + `web/index.en.html` are an independent bundle checked by `scripts/check-en-web.py`; the English attachment send path and `file.attach` 60 s timeout are restored.
- `scripts/ci.sh` now checks generated-source consistency, Web bundle structure and boolean-only `gates`.

### Documentation
- Added the code-accumulation audit (Chinese and English) and the rc.4.6 change notes.
- Archived `Windows真机复测报告-v1.2.1-rc.4.5.md`; the report index now has 12 reports.
- Linux suite: 206 items, 205 passed, 1 ignored. Release bench three runs: kernel 2,578,496 B; warm P50 1.276/1.275/1.285 ms; cold P50 4.469/5.033/5.272 ms; all boolean gates true, `write_gate_status:"pass"`, `write_gate_raw:true`, `reliability_basis:"mem=17%,cpu=68%"`.

## [1.2.1-rc.4.5] - 2026-11-26

### Fixed
- N29: explicit write-file tasks no longer require guide hard criteria. `parse_rubric_with_input` keeps judge-only items and strips `must_include` / `min_points` / `pattern` and related deterministic keys when `file_task` is true, so `must_include:["-"]` no longer trips the two-character term rule. `review_prepare` skips the zero-hard-criteria retry for file tasks. Tests: `review_rubric_file_task_bypasses_symbol_term_limit`; the write-task integration fixture now reproduces the N29 bad-symbol rubric and still requires `approved` with judge `votes>0`.
- N30: on Windows, a write P95 of 10–20 ms is `advisory` instead of a red gate; above 20 ms still fails. `bench --json` gains `write_gate_status` and `write_gate_raw`, and the platform write key uses the effective result. Linux keeps the 100 µs limit. Test: `write_gate_marks_windows_edge_jitter_as_advisory`; CI validates both new fields.
- N31: the large-1 L17 list scorer now recognises `**1.**` bold numbering in addition to `1.`, `-` and `*`. This is a test-scoring change, not a product change.

### Documentation
- Archived `Windows真机复测报告-v1.2.1-rc.4.4.md` and updated both report indexes to 11 reports.
- Added `docs/model-test-reports/exam-papers/` with five papers (62 questions / 171 points), a corrected index and no local runner or historical results.
- Linux suite: 201 tests, 200 passed, 1 ignored. Release bench three runs: kernel 2,568,840 B; warm P50 1.182/1.194/1.176 ms; cold P50 3.413/3.239/3.160 ms; all required gates true, `write_gate_status:"pass"`, `write_gate_raw:true`.

## [1.2.1-rc.4.4] - 2026-11-26

### Fixed
- N23: `Server::stop` now retries the wake connection for a 3 s budget at 20 ms intervals, and bench warm/RPC joins poll `is_finished` with a 5 s deadline instead of joining forever. `scripts/ci.sh` wraps `bench --json` in `timeout 90` and validates the seven release gates.
- N24: explicit write-file tasks route all guide criteria to the judge, strip deterministic keys, and carry `tool.fs.write` success plus file content as evidence. A successful write followed by malformed XML/empty output falls back to a deterministic written-path answer instead of `failed`. Tests: `review_rubric_moves_file_task_terms_to_judge`, `review_rubric_keeps_hard_items_for_code_tasks`, `write_fallback_names_the_file_without_claiming_missing_content`; integration requires judge `votes>0`.
- N25: XML function open tags missing `>` are cut at the newline or parameter marker, and parameter open tags tolerate the same omission. Test: `malformed_xml_function_open_without_gt_is_recovered`.
- N26: comparison answers must cover each requested axis across mechanism/connection, reliability/consistency, performance/overhead and scenarios, one line per dimension. Test: updated `system_prompt_distinguishes_brevity_from_coverage`.
- N27: `BenchTempDir` removes bench temp dirs on Drop; startup clears `fa-bench-*` older than 24 hours without touching fresh directories.
- TUI: `draw` now takes `&mut App` and clamps `chat_scroll` to `max_scroll`, so overscrolling at the top no longer delays the next Down. Test: `chat_scroll_is_clamped_after_overscrolling_at_top` in both TUIs.

### Documentation
- N28 (Q7 bold headings not recognized by the list scorer) is recorded as a test-side limitation, not a product change.
- Windows x64 cross-compiles ten exes with system-DLL-only imports; Wine 6.0.3 passed `version` and three `bench --json` runs with a temporary `bcryptprimitives` stub, while cold-spawn/RSS are not a Windows baseline.
- Linux suite: 198 tests, 197 passed, 1 ignored. Release bench three runs: kernel 2,568,520 B; warm P50 1.221/1.198/1.174 ms; cold P50 3.567/3.060/3.099 ms; all gates true.

## [1.2.1-rc.4.3] - 2026-11-25

### Fixed
- N19: `serve` used the retrying `ipc::connect` to detect an existing kernel, so a clean install root waited the full 3-second pipe-retry deadline before starting and appeared as a fixed ~3.18 s cold spawn. Added one-shot `ipc::try_connect`; `serve`, `wait_ready`, the smoke probe and `Server::stop` use it, while normal clients keep the bounded retry. Test: `try_connect_does_not_enter_the_retry_deadline`.
- N20: the system prompt now distinguishes content answers from file creation. Code, text, plans and translations go in the response by default; file tools run only when the user explicitly asks to save, write or create a file. Test: `system_prompt_separates_content_output_from_file_creation`.
- N21: list/comparison answers must map their first-level items to the requested axes; when the user gives no axes, cover mechanism, reliability, performance/overhead and scenarios instead of repeating one dimension with synonyms. Test: `system_prompt_distinguishes_brevity_from_coverage`.
- N22: explicit write-file tasks now give review-gate the successful `tool.fs.write` path and content as labelled evidence, so the judge checks the file instead of demanding a verbatim final answer. The prompt also stops repeated post-write reads. Tests: `review_file_evidence_is_only_for_explicit_write_tasks`, `review_answer_includes_written_file_content_before_final_answer`, `review_gate_validates_written_file_content_instead_of_answer_wording`.

### Added
- `docs/model-test-reports/`: nine curated Windows real-machine reports with reusable local-model data, plus CSVs for the rc.4.1 four-task comparison, the rc.4.2 seven-question exam and the rc.2 → rc.4 review-gate milestones.

## [1.2.1-rc.4.2] - 2026-11-25

### Fixed
- N15: the context parser recovers Qwen/Ollama XML-style function/parameter tool calls (both `=` and `name="..."` forms, values until the parameter close tag, missing function close tolerated), and the Ollama provider salvages a parsed call from the accumulated stream instead of terminating the turn on the XML error. Tests: `xml_function_parameter_fallback`, `malformed_xml_function_without_close_is_recovered`.
- N16: Windows `ipc::connect` retries `ERROR_FILE_NOT_FOUND(2)` and `ERROR_PIPE_NOT_CONNECTED(233)` as well as `ERROR_PIPE_BUSY` within its 3-second deadline; `bench` creates the `rpc` directory explicitly and retries `Client::connect` for up to 10 seconds after `wait_ready`. Linux behavior is unchanged.
- N17: `bench --json` no longer puts `boot_gate_trusted` in `gates`; it moves to `advisory_gates` with `boot_gate_status` and `reliability`, so a low-reliability host is not counted as a failed release gate. `metrics` keeps the field for diagnostics; human output prints the advisory line separately.
- N18: the system prompt keeps default brevity but requires full coverage when the user asks to list, compare, enumerate, or explain all points. Test: `system_prompt_distinguishes_brevity_from_coverage`.

## [1.2.1-rc.4.1] - 2026-11-25

### Fixed
- N10: path/file `must_include` values use a 64-character limit (`PATH_MUST_INCLUDE_MAX_CHARS`) while ordinary terms keep 12; `workspace/notes.txt` is accepted, an overlong path is rejected with a `pattern/regex` suggestion.
- N11: plain-text `tool_call <name> <json>` output is recovered by the context parser; `tool_fs_read`/`tool_fs_write`/`tool_fs_list`/`tool_shell` aliases normalize to standard names, and review feedback carries a tool-format guard.
- N12: search/news/fetch results are also written to `data/web-artifacts/<session>/` as `audit_artifact`; workspace `artifact` remains.
- N13: an accepted rubric is written to `data/review-artifacts/<session>/<ms>-rubric-final.json` with the normalized `match`/`min_matches`; `review.guide` gains `final_artifact`.

### Changed
- Documentation records N14: multi-task long flows need a ≥7B exec model and repeated sampling; 4B results are observational, not a regression gate.

## [1.2.1-rc.4] - 2026-11-25

### Fixed
- N5: `must_include` now supports `match: "any" | "all" | "min"` plus `min_matches`. A multi-term item defaults to `min` with `ceil(n/2)` required matches instead of requiring every literal; protocol constraints can opt into `all`, and `any` covers one-of lists. Hardcheck reports `{hits}/{total}` and no longer fails a semantically correct paraphrase just because all terms are not present.
- N6: subjective criteria (`准确/准确无误/完整/正确/恰当/专业/规范/流畅/自然`, plus the existing readability words) may not use `must_not_include`; a banned word that already appears in the user input also rejects the guide. This prevents “术语准确” from banning `软件/硬件/计算机` and failing every correct answer. N4's fixture-item drop remains covered by the rc.3.5 raw regression test.
- N7: bench uses a unique temporary directory per run (`fa-bench-<pid>-<random>`), so a recycled Windows PID or stale directory cannot produce `os error 2`; I/O failures carry the operation and path, and `bench --json` returns `{"ok":false,"code":"artifact_missing"|"bench_failed","error":...}`.
- N8: identical tool calls in one model reply are de-duplicated before execution; every `tool.call` event carries `attempt` and `call_index`, so a retry-chain replay is distinguishable instead of silently executing twice.
- N9: when `metrics.env.reliability` is not `high`, the boot gate is marked `boot_gate_status:"advisory"` with `boot_gate_raw` preserved; `gates.boot_lt_*` is no longer a red failure caused only by host memory pressure. `reliability` is copied into gates and metrics.

### Added
- Web search/news/fetch results are persisted as pretty JSON under `workspace/<project>/.featheagent/search/<timestamp>-<tool>.json`; the tool result gains `artifact` and `artifact_kind`, and a write failure is reported as `persist_error` without failing the search.
- WebUI file attachment: a `+` button opens the native file picker, uploads bytes over HTTP RPC (`file.attach`, base64, 8 MiB file limit / 12 MiB request limit), and shows removable chips with the kernel-returned path. TUI supports `@<path>` and `@"path with spaces"`; it checks `is_file` before calling `file.prepare`.
- Kernel `file.attach` / `file.prepare`: rechecks existence, copies files outside the install root to `workspace/default/uploads/`, and returns the model-facing path; browser uploads always land in the default workspace. Tests: upload to default workspace, outside-path copy/inside-path passthrough, attachment parser (plain/quoted/email), base64 round-trip, search artifact JSON.

### Changed
- `GUIDE_SYSTEM` explains `match`/`min_matches` and forbids subjective `must_not_include`.
- `bench` JSON adds `boot_gate_raw`, `boot_gate_trusted`, `boot_gate_status`, `reliability`.

## [1.2.1-rc.3.6] - 2026-11-25

### Fixed
- N1: `must_include` is term-only. `GUIDE_SYSTEM` allows proper nouns, terms, file names, commands or fixed forms of 2–12 characters and rejects conceptual descriptions, sentences and connection words (`和/与/及/或/是/的/了`). `MUST_INCLUDE_MAX_CHARS` is 12; `must_not_include` keeps a separate 20-character limit. `review.hardcheck` splits the phrase into terms at whitespace, `、`, `，`, `,`, `；`, `;` and `/`, and requires every term; the reported `管理计算机硬件和资源` is rejected at guide time, while `管理计算机硬件 资源` can match a paraphrase.
- N2: `util::percentile` sorts its own samples. Bench computes `boot_p50`/`boot_p90` once, reuses both, and exposes `metrics.boot_percentiles_consistent` and `gates.metrics_consistent`; P50 ≤ P90 is now structural.
- N3: `run_turn` retries empty responses twice (temperatures 0.70/0.75) and malformed Ollama tool calls twice with a JSON-format correction, each emitting `model.retry` (`reason:"empty"` / `"tool_format"`). A plugin crash still restarts once; `failed` is emitted only after the budget.
- N4: a rubric item whose `must_not_include` contains `REVIEW-FINAL`, `REVIEW-START`, `<...>`, `__FILL`, `{{` or `}}` is dropped as one item with `review.warn`.

### Added
- English editions of all five top-level binaries: `featheragent-en`, `featheragent-tui-en`, `featheragent-webui-en`, `feather-plugin-en`, `feather-storage-en`. The `-en` suffix selects English assets in the WebUI; kernel/TUI use separate English source targets; plugin/storage are protocol-only aliases.
- Windows zip contains ten exes (five Chinese + five `-en`); Linux core contains eight binaries (four Chinese + four `-en`); the WebUI package contains both language editions.
- Tests: `percentile_works_on_unsorted_samples_and_p50_le_p90`, `tool_format_error_detector_covers_ollama_and_json_errors`, `review_rubric_rejects_conceptual_must_include_with_connective`, `review_rubric_rejects_one_char_must_include`, `review_hardcheck_matches_include_terms_across_wording`, `review_rubric_drops_fixture_must_not_include_item`, `english_suffix_selects_english_assets`.

### Changed
- Linux automation: **162 test executions, 161 passed, 1 ignored**; the English worker/WebUI alias targets repeat 26 plugin and 4 WebUI cases. `fmt` and `clippy -D warnings` pass.
- Bench: warm P50 1.142/1.167/1.163 ms, P90 1.761/1.190/1.170 ms; cold 3.722/2.324/2.262 ms; all gates true.
- No wire message, contract, provider, storage, tool or plugin setting changed. `bench` fields are additive.

## [1.2.1-rc.3.5] - 2026-11-25

### Fixed
- M1: the guide no longer turns placeholder examples into criteria. `GUIDE_SYSTEM` uses `__FILL_*__` tokens, forbids discussing placeholders/templates inside a criterion, and `review.parse_rubric` drops self-referential placeholder items while keeping real ones. The a2 rubric now keeps `c1`/`c3`, drops `c2`, emits `review.warn`, and no longer degrades the round.
- M1: `must_include` accepts a string array (`["SYN","ACK"]`) and hardcheck requires every phrase instead of treating the item as description-only.
- M3: subjective description items (concise, clear, well-formatted, …) are marked `advisory` and excluded from both hardcheck and judge, with a `review.warn` note. The kernel also rejects a rubric that has no executable criterion at all after retries, instead of accepting it because it is non-empty.
- M4: the S3 t6 prompt now explicitly requires one `tool.fs.read` call and forbids answering from guesswork.

### Added
- `review.parse_rubric` returns `advisory` ids; `review.warn` reports dropped placeholder items and subjective downgrades.
- Tests: placeholder-meta stripping, subjective advisory marking, array `must_include`, plus integration `review_gate_drops_placeholder_meta_item_and_approves` and `review_gate_subjective_criterion_is_advisory`.

### Changed
- `GUIDE_SYSTEM` rules 8/9: examples only show field names, and subjective criteria are disallowed in favour of countable checks.
- No wire message, contract, provider, storage, tool or plugin setting changed.

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
