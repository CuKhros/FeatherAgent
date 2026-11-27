# 本地模型测试报告归档

这个目录保存 Windows 真机复测中**含有可复核本地模型测试数据**的报告。它是精选证据集，不是全部回归报告的堆叠。

## 收录标准

满足以下任一项才收录：

- 有“同一模型裸跑 vs FeatherAgent 环境”的受控对照；
- 有逐题得分、耗时、输出长度或重复次数等可复核数据；
- 有本地 `guide` / `exec` / `judge` 模型的多轮行为记录，包含模型名、温度、状态和失败原因。

只包含构建缺陷、UI 缺陷清单的复测报告不收录：它们属于回归历史，不属于模型测试数据。

## 测试环境

| 项 | 值 |
|---|---|
| 机器 | Windows x64 真机 |
| 运行时 | Ollama 0.32.9 |
| 执行模型 | `qwen3.5:4b` |
| Guide / Judge 模型 | `qwen2.5-coder:7b` |
| 传输 | 本地 Ollama HTTP API；测试时显式配置代理 |

## 入选报告

| 报告 | 核心模型数据 | 入选理由 |
|---|---|---|
| [v1.2.1-rc.2](reports/Windows真机复测报告-v1.2.1-rc.2.md) | 首次真实模型 review-gate：全 4B 配置 3/3 degraded；7B guide/judge + 4B exec 走满 3 轮但最终 unapproved | 建立最初的 real-model 失败基线 |
| [v1.2.1-rc.2.1](reports/Windows真机复测报告-v1.2.1-rc.2.1.md) | 7B guide/judge + 4B exec 在 2 轮内 `approved` | 首次真实模型通过，同时暴露 `must_include` 整句照抄问题 |
| [v1.2.1-rc.3](reports/Windows真机复测报告-v1.2.1-rc.3.md) | 更严格的 rubric schema 下 4/4 degraded | 证明“尺子做得更准”可能超出 7B 模型的 schema 能力 |
| [v1.2.1-rc.3.1](reports/Windows真机复测报告-v1.2.1-rc.3.1.md) | 3 次运行：1 approved、2 unapproved；`min_points` 误杀 | 第一次出现相对均衡的真实模型结果 |
| [v1.2.1-rc.3.5](reports/Windows真机复测报告-v1.2.1-rc.3.5.md) | TCP 任务 2/2 一轮通过，judge 未参与；S3 触发 5/5、t6 单次调用 4/5 | 目前成本最低、最干净的一次 review-gate 通过 |
| [v1.2.1-rc.3.6](reports/Windows真机复测报告-v1.2.1-rc.3.6.md) | 4 次畸形调用尝试中 3 次靠 `model.retry` 恢复；1 次因连续空响应耗尽预算 | 量化小模型重试链的实际收益 |
| [v1.2.1-rc.4](reports/Windows真机复测报告-v1.2.1-rc.4.md) | review-gate 开/关多任务对照：OFF 3/4、1/4；ON 0/4、0/4；ON 耗时是 OFF 的 3.2–4.2 倍 | 量化门禁成本与 4B 波动 |
| [v1.2.1-rc.4.1](reports/Windows真机复测报告-v1.2.1-rc.4.1.md) | 代码/设计/翻译/答题四类：15/16 vs 15/16；总耗时 49s vs 104s | 受控的裸跑 vs FA 能力对照 |
| [v1.2.1-rc.4.2](reports/Windows真机复测报告-v1.2.1-rc.4.2.md) | 七题 24 分试卷：24/24 vs 17/24；143s vs 332s | 更大的混合任务对照，包含两次裸跑空响应 |
| [v1.2.1-rc.4.3](reports/Windows真机复测报告-v1.2.1-rc.4.3.md) | 七题 24 分试卷：22/24 vs 11/24；123s vs 335s；裸跑 Q4–Q7 在 50–66 秒后空响应 | 确认代码答复形态，同时暴露写文件 judge 假阴性与 bench 卡死（阻断 N19） |
| [v1.2.1-rc.4.4](reports/Windows真机复测报告-v1.2.1-rc.4.4.md) | 大试卷 1（20 题 / 54 分）：50.5/54 vs 30.0/54；319s vs 856s；裸跑 8 题空响应；bench 47.1ms、冷启动 214.1ms | 确认 N23/N19 已修，并暴露写文件任务 N29 的规则冲突 |
| [v1.2.1-rc.4.5](reports/Windows真机复测报告-v1.2.1-rc.4.5.md) | 大试卷 2（20 题 / 60 分）：48.0/60 vs 45.0/60；1,653s vs 1,059s；judge 因 Windows 路径非法转义 3/3 degraded；FA 两题截断为 136 / 390 字 | 区分“避免空响应”与“内容质量”，并暴露 N32–N35 |
| [v1.2.1-rc.4.6](reports/Windows真机复测报告-v1.2.1-rc.4.6.md) | 数学小卷 19.5/25 vs 16.5/25；大卷 40.7/56 vs 31.7/56；大卷 FA 1 题空响应 vs 裸跑 5 题；N32–N35 真机通过；N36 的 9 字 evidence 被 10 字门限误杀 | 首次数学双卷对照，确认四项修复，暴露 N36–N39 |
| [v1.2.1-rc.4.7](reports/Windows真机复测报告-v1.2.1-rc.4.7.md) | 数学小卷 21.5/25 vs 14.5/25；大卷 41.6/56 vs 32.3/56；放行 `tool.shell` 后 FA 耗时 126s / 446s；N37、N38 真机确认；N36 因残留 turn 连续 `interrupted` 未取得读数；暴露 N40/N41 | 量化放行 shell 的分数与耗时收益；把公式写法与分步判据噪声单独暴露出来 |
| [v1.2.1-rc.4.8](reports/Windows真机复测报告-v1.2.1-rc.4.8.md) | ML02/ML10 定向：N40 3/3、N41 按内容 4/4；N42 5/5 干净；暴露 N43/N44/N45；ML10 长度跨度 140–3514 | 确认公式、分步与残留 turn 修复；分离 judge 空 evidence、畸形 XML 尾与自我推翻 |

共 15 份报告保留原始中文表述。公开前已替换本机 Windows 用户名；命令、端口、模型名和事件记录都未改动。

## 试卷库

[`exam-papers/`](exam-papers/) 收录 7 份可复用试卷（`small-a`、`small-b`、`small-c`、`large-1`、`large-2`、`math-small`、`math-large`），共 90 题、满分 252 分；只包含试卷 JSON 与格式说明，不包含运行器或历史结果。各卷题数/满分以 JSON 的 `full_score` 为准。

## 结构化数据

| 文件 | 内容 |
|---|---|
| [`data/small-model-comparison.csv`](data/small-model-comparison.csv) | rc.4.1 的四类任务得分、耗时和答案长度 |
| [`data/seven-question-exam.csv`](data/seven-question-exam.csv) | rc.4.2 的逐题得分与耗时 |
| [`data/review-gate-real-model-runs.csv`](data/review-gate-real-model-runs.csv) | rc.2 至 rc.4 的真实模型 review-gate 里程碑 |
| [`data/large-2-rc.4.5.csv`](data/large-2-rc.4.5.csv) | 大试卷 2 的 20 题逐题分数、耗时与答案长度（裸跑 vs FeatherAgent） |
| [`data/math-papers-rc.4.6.csv`](data/math-papers-rc.4.6.csv) | 数学小卷/大卷 28 题的逐题分数（裸跑 vs FeatherAgent） |
| [`data/math-papers-rc.4.7.csv`](data/math-papers-rc.4.7.csv) | 同一 28 题在 rc.4.7 放行 shell 后的逐题分数，附 N37/N38/N40/N41 备注 |
| [`data/math-papers-rc.4.8-targeted.csv`](data/math-papers-rc.4.8-targeted.csv) | rc.4.8 的 ML02/ML10 定向行，含 status、输出长度与 N44/N45/N46 备注 |

## 读数据时要注意

- rc.4.1 与 rc.4.2 的分数是规则化覆盖面评分，不是完整正确性评分；Q1 的 LRU 代码没有实际执行。
- rc.4.2 的 Q1 暴露了交付形态问题：FA 用 `tool.fs.write` 把代码写进文件，没有直接返回代码块；关键词评分仍给了满分。
- 裸跑 Q6/Q7 是空响应，FA 靠空响应重试恢复。这是被测 harness 带来的真实差异，不是人工补答。
- review-gate 的样本量很小，4B 本身不稳定；表中的次数只是已记录运行的观察值，不是统计显著结论。
- 模型名和结果只代表当时的测试记录，不代表其他机器或后续模型版本的表现。
