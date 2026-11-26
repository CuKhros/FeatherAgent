# FeatherAgent v1.2.1-rc.4.6 — Code-Accumulation Audit

> Audited revision: `v1.2.1-rc.4.5` `main` (`4dc2ded`)
> Comparison baselines: the generator shape at `v1.2.1-rc.3.6`, the review parser boundary, and the `bench --json` shape
> Date: 2026-11-26
>
> This document answers one question: **where did rc.4.5 stop changing the architecture and start stacking special cases on the old structure?** This round only refactored the places with hard evidence that also relate to N32–N35. File length alone was not treated as debt, and there was no whole-repository rewrite.

## 1. Criteria

A location is listed as accumulated debt only when it meets at least one of these tests and points to concrete evidence:

1. **One semantic has multiple implementation paths**, and one is already unreachable.
2. **A generated file is edited by hand**, the generator has failed for several versions, and source and artifact are no longer isomorphic.
3. **One object mixes several semantics**, so consumers must guess by convention and any new field can be misread.
4. **One constant is hard-coded at multiple call sites**, with no config field, so behaviour changes require code changes.
5. **Chinese/English or front-end/back-end copies are maintained separately** without structural checks, so features drift on one side.

A long function or a large file is not automatically debt; it becomes debt when it also carries parsing, validation, retries and prompt assembly, so the next fix must add another branch.

## 2. Five real accumulation points

### P1 (highest) The Chinese/English Rust UI generator failed; generated files became hand-maintained

`scripts/gen-en-ui.py` last passed at `v1.2.1-rc.3.6`:

| Version | Chinese literals in `tui.rs` | Chinese literals in `kernel.rs` | Translation table | Generator |
|---|---:|---:|---|---|
| rc.3.6 | 166 | 72 | 166 / 72 | pass |
| rc.4 | 170 | 74 | not updated | fail |
| rc.4.5 | 170 | 76 | not updated | fail |

The generator asserts that the set of Chinese literals equals the number of translations. New Chinese strings were added from rc.4 onward without translations, so the assertion failed; after that `tui_en.rs` and `kernel_en.rs` became **hand-maintained generated files**. The rc.4.5 N30 fix even inlined a separate implementation in `kernel_en.rs`, so it no longer mirrored `kernel.rs`.

This is not a missing-translation issue. It means **the same UI logic has two sources of truth**: either the Chinese source or the English artifact can be edited, and every later Chinese change widens the fork.

Treatment: rebuild `scripts/en-ui/tui_en.py` and `kernel_en.py` from the rc.3.6 166/72 mapping in the current `sorted(zh)` order, add the rc.4/rc.4.5 entries, and regenerate. Both outputs now have no Chinese residue. `gen-en-ui.py` only generates Rust UI and no longer pretends to generate WebUI. CI now fails if regeneration produces a diff, so hand edits cannot land.

### P2 The review parser: 3,028-line file, ~430-line function, unreachable branch

`src/bin/plugin.rs` was 3,028 lines and `parse_rubric_with_input` was about 430 lines. N29 added a `file_task` branch at the top that `continue`s, but the second half of the function still had a full `if file_task { ... } else { ... }` block, so the `file_task` branch there was unreachable. The N29 fix landed while the pre-N29 branch stayed behind to be maintained.

Treatment: extract `src/review.rs` (924 lines) for pure functions only: tolerant JSON, rubric normalization, placeholder/subjective detection, `parse_rubric`, `parse_rubric_with_input`, `parse_verdict` and `evidence_quotes`. `plugin.rs` keeps only adapter calls and drops from 3,028 to 2,219 lines (-809). `parse_rubric_with_input` now builds the base `norm` once per item, keeps one early `file_task` branch, and handles only normal tasks in the second half; the unreachable second `file_task` branch is gone.

### P3 `bench --json` `gates` mixed verdicts, status, thresholds and reliability

rc.4.5 `gates` contained:

- booleans: `binary_le_8mb`, `rss_le_15mb`, `*_lt_*`;
- a platform string: `platform`;
- a reliability string: `reliability`;
- status strings: `boot_gate_status`, `write_gate_status`;
- raw-reading booleans: `boot_gate_raw`, `write_gate_raw`;
- thresholds: `boot_limit_ms`, `write_limit_us`.

A consumer that counted `false` values in `gates` treated `write_gate_raw=false` as a failure — that is N34. The root cause is not naming; the object had lost its semantic boundary.

Treatment: `gates` now contains only boolean verdicts; new `diagnostics` holds platform, raw readings, status, thresholds, reliability and modes; `advisory_gates` holds advisory items. `scripts/ci.sh` asserts every `gates` value is boolean and reads `write_gate_status/raw` from `diagnostics`.

### P4 Output limit 512 hard-coded at two call sites, with no config field

`ModelProfile.max_output` was hard-coded to 512 in two engine call sites; `Config` had no output-limit field and provider requests never sent `num_predict`/`max_tokens`. A local model template can impose another small default. N33's M15 produced only 136 characters and M20 only 390; 390 Chinese characters is roughly 500 tokens, matching the 512 cap.

Treatment: `Config.max_output` defaults to 2048 and accepts 128–8192; both `ModelProfile` sites read `cfg.max_output`; `run_model` and review calls pass it to the provider; Ollama receives `options.num_predict`, OpenAI-compatible endpoints receive `max_tokens`. Normal chat `ModelMessage` gains `truncated`, and a review role that ends with `finish=length` now fails and retries instead of accepting a half answer.

### P5 The English WebUI is a hand-maintained bundle and had already lost attachment sending

`web/app.en.js` and `web/index.en.html` are separate hand-maintained bundles; the Web part of `gen-en-ui.py` also failed on the same string-count mismatch. The audit also found that the English `send()` lacked attachment sending, so the English package would drop attachments — the classic two-copy drift with no structural check.

Treatment: WebUI English is explicitly a hand-maintained bundle; `gen-en-ui.py` no longer touches Web. New `scripts/check-en-web.py` checks four things: no Chinese literals in English JS, identical function names/element ids/RPC methods, identical element ids between English and Chinese HTML, and `lang="en"` plus an app-script reference. Attachment sending, the `file.attach` 60 s timeout and three Chinese leftovers were restored.

## 3. What was deliberately left alone

These are long but did not meet the criteria in §1, or the payoff was lower than the regression risk:

| Location | Lines | Decision |
|---|---:|---|
| `src/engine/mod.rs` | 3,712 | Session, tool loop, review orchestration and prompts share one module; there is room to split, but no unreachable-branch or duplicate-implementation evidence. Leave for a dedicated refactor round |
| `src/bin/tui.rs` | 3,222 | TUI state machine and rendering are coupled; this round only restored the generation chain |
| `src/bin/plugin.rs` | 2,219 | Parser layer extracted; provider/tool/review roles still share one binary; next step is role-based modules |
| `src/review.rs` | 924 | New boundary; keep it stable for one round |
| `web/app.en.js` | 1,134 | Hand bundle + structural check is the lowest-cost option; runtime i18n is a larger product decision |

## 4. Results and verification

- `src/bin/plugin.rs` 3,028 → 2,219 lines; `src/review.rs` 924 lines.
- `scripts/gen-en-ui.py` regenerated `tui_en.rs` (3,207 lines) and `kernel_en.rs` (1,458 lines); two consecutive runs produced identical hashes and no Chinese residue.
- `scripts/check-en-web.py` prints `WebUI English bundle OK`.
- `scripts/ci.sh` now checks generated-source consistency, the Web bundle structure and boolean-only `gates`.
- Linux suite: **206 items, 205 passed, 1 ignored** (`local_web_search_tool_via_kernel`, needs public network). The rc.4.5 baseline was 201 items (200 passed + 1 ignored), a net gain of five passing tests.
- N32/N33/N35 have not been retested against real Ollama/Windows; Wine and Linux unit tests are not real-hardware evidence.

## 5. Follow-up

1. If `plugin.rs` is split again, split by role into `provider/`, `tools/` and `review_adapter/` rather than adding another `match` arm to one file.
2. Split `engine/mod.rs` along session loop / model call / review orchestration / prompt boundaries, with regression coverage first.
3. Keep the English WebUI as either a checked hand bundle or a runtime i18n implementation; do not return to “half generated, half hand-edited”.
4. Before adding any `bench` field, decide whether it is a verdict, a diagnostic or an advisory. Putting it in the wrong object creates the next N34.
