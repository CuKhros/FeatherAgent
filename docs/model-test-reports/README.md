# Local Model Test Reports

This directory preserves Windows real-machine reports that contain reusable local-model evaluation data. It is a curated evidence set, not a dump of every regression report.

## What is included

A report is included when it contains at least one of the following:

- a controlled comparison between a bare local model and the same model running inside FeatherAgent;
- per-task scores, wall-clock timings, output lengths, or repeat counts that can be re-checked;
- a multi-run behavioral trace of the local `guide` / `exec` / `judge` models, with model names, temperatures, status, and failure reasons.

Reports that only contain build/UI defect checklists are intentionally excluded. Their value is regression history, not model evaluation data.

## Test environment

| Item | Value |
|---|---|
| Machine | Windows x64 real machine |
| Runtime | Ollama 0.32.9 |
| Exec model | `qwen3.5:4b` |
| Guide / judge model | `qwen2.5-coder:7b` |
| Transport | Local Ollama HTTP API; proxy configured explicitly during the runs |

## Curated reports

| Report | Main model data | Why it is kept |
|---|---|---|
| [v1.2.1-rc.2](reports/Windows真机复测报告-v1.2.1-rc.2.md) | First real-model review-gate run: 4B-only 3/3 degraded; 7B guide/judge + 4B exec reaches 3 rounds but is unapproved | Establishes the initial real-model failure mode |
| [v1.2.1-rc.2.1](reports/Windows真机复测报告-v1.2.1-rc.2.1.md) | 7B guide/judge + 4B exec reaches `approved` in 2 rounds | First successful real-model gate, and the `must_include` sentence-copy problem |
| [v1.2.1-rc.3](reports/Windows真机复测报告-v1.2.1-rc.3.md) | 4/4 degraded after a stricter rubric schema | Shows that a stricter grader can exceed a 7B model's schema capability |
| [v1.2.1-rc.3.1](reports/Windows真机复测报告-v1.2.1-rc.3.1.md) | 3 runs: 1 approved, 2 unapproved; `min_points` false negatives | First balanced real-model gate result |
| [v1.2.1-rc.3.5](reports/Windows真机复测报告-v1.2.1-rc.3.5.md) | TCP task approved 2/2, both in round 1 with no judge call; S3 5/5 triggered; t6 4/5 single-call | Cleanest low-cost review-gate pass |
| [v1.2.1-rc.3.6](reports/Windows真机复测报告-v1.2.1-rc.3.6.md) | 4 malformed-call attempts, 3 recovered by `model.retry`; one exhausted after repeated empty responses | Measures retry value on a small model |
| [v1.2.1-rc.4](reports/Windows真机复测报告-v1.2.1-rc.4.md) | Review-gate ON/OFF multi-task A/B: OFF 3/4 and 1/4; ON 0/4 and 0/4; ON 3.2–4.2× slower | Quantifies gate cost and 4B variance |
| [v1.2.1-rc.4.1](reports/Windows真机复测报告-v1.2.1-rc.4.1.md) | Four task classes, 4 points each: 15/16 vs 15/16; 49 s vs 104 s | Controlled bare-vs-FA comparison |
| [v1.2.1-rc.4.2](reports/Windows真机复测报告-v1.2.1-rc.4.2.md) | Seven-question exam, 24 points: 24/24 vs 17/24; 143 s vs 332 s | Larger mixed-task comparison, including two bare-model empty responses |
| [v1.2.1-rc.4.3](reports/Windows真机复测报告-v1.2.1-rc.4.3.md) | Seven-question exam, 24 points: 22/24 vs 11/24; 123 s vs 335 s; bare Q4–Q7 returned empty after 50–66 s | Confirms answer-only code delivery and exposes the write-task judge gap, plus a bench hang that blocked N19 |
| [v1.2.1-rc.4.4](reports/Windows真机复测报告-v1.2.1-rc.4.4.md) | Large exam 1 (20 questions, 54 points): 50.5/54 vs 30.0/54; 319 s vs 856 s; 8 bare-model empty responses; bench 47.1 ms, cold start 214.1 ms | Confirms N23/N19 are fixed and exposes the N29 rule conflict on write-file tasks |

Eleven reports are preserved in their original Chinese wording. Local Windows usernames were replaced before publication. Commands, ports, model names, and event traces are unchanged.

## Exam papers

[`exam-papers/`](exam-papers/) contains five reusable papers (`small-a`, `small-b`, `small-c`, `large-1`, `large-2`): 62 questions and 171 points in total. It ships only the paper JSON and a format note, with no runner or historical results. Per-paper counts and full scores follow the `full_score` field in each JSON.

## Extracted data

| File | Contents |
|---|---|
| [`data/small-model-comparison.csv`](data/small-model-comparison.csv) | Four task classes from rc.4.1, with scores, latency, and answer lengths |
| [`data/seven-question-exam.csv`](data/seven-question-exam.csv) | Per-question scores and latency from rc.4.2 |
| [`data/review-gate-real-model-runs.csv`](data/review-gate-real-model-runs.csv) | Real-model review-gate milestones from rc.2 through rc.4 |

## Caveats

- Scores in rc.4.1 and rc.4.2 are rule-based coverage scores, not full correctness grades. The Q1 LRU code was not executed.
- rc.4.2's Q1 result exposed a presentation failure: FA wrote the code with `tool.fs.write` instead of returning a code block. Keyword scoring still counted it as full marks.
- Q6 and Q7 in the bare-model run returned empty responses. FeatherAgent recovered through its empty-response retry path; this is part of the measured difference, not an external correction.
- The review-gate runs are small samples. The 4B model is nondeterministic, so the counts are observations from the recorded runs, not statistically robust estimates.
- Model names and results are historical test records. They are not performance claims about other machines or newer model builds.
