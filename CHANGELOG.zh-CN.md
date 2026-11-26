# Changelog

格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本遵循语义化版本。`v1.2.1-rc.4.5` 是当前公开仓库候选版，尚未替代稳定版。

## [1.2.1-rc.4.5] - 2026-11-26

### 修复
- N29：明确写文件任务不再要求 guide 产出硬判据。`parse_rubric_with_input` 在 `file_task` 时只保留 judge-only 条目，剥掉 `must_include` / `min_points` / `pattern` 等确定性键，`must_include:["-"]` 不再触发“术语至少 2 字”；`review_prepare` 对写文件任务跳过“零硬判据”重试。测试：`review_rubric_file_task_bypasses_symbol_term_limit`；写文件集成夹具改为 N29 坏符号判据，仍要求 `approved` 且 judge `votes>0`。
- N30：Windows 上写 P95 在 10–20 ms 之间标 `advisory`，不再整轮判红；超过 20 ms 仍 fail。`bench --json` 新增 `write_gate_status` 与 `write_gate_raw`，平台写 key 使用 effective 结果；Linux 仍按 100 µs。测试：`write_gate_marks_windows_edge_jitter_as_advisory`；CI 校验这两个新字段。
- N31：large-1 的 L17 列表判据在 `1.`、`-`、`*` 之外识别 `**1.**` 粗体编号。这是测试评分规则改动，不是产品改动。

### 文档
- 归档 `Windows真机复测报告-v1.2.1-rc.4.4.md`，两份报告索引改为 11 份。
- 新增 `docs/model-test-reports/exam-papers/`：5 份试卷（62 题 / 171 分）、修正后的索引；本机 runner 与历史结果不入库。
- Linux 全量 201 项、200 通过、1 ignored。Release bench 三连跑：内核 2,568,840 B；热启动 P50 1.182/1.194/1.176 ms；冷启动 P50 3.413/3.239/3.160 ms；必需门禁全绿，`write_gate_status:"pass"`、`write_gate_raw:true`。

## [1.2.1-rc.4.4] - 2026-11-26

### 修复
- N23：`Server::stop` 改为 3 秒总预算、20 ms 间隔重试唤醒连接；bench warm/RPC join 改 5 秒 deadline 轮询，不再无限等待。`scripts/ci.sh` 用 `timeout 90` 跑 `bench --json` 并校验 7 个发布 gate。
- N24：明确写文件任务把 guide 判据全部转 judge_only，剥掉确定性键；证据带 `tool.fs.write` 成功记录与文件正文。写成功后模型仍返回畸形 XML/空响应时，回退为“已写入 path”的确定答复，不再整轮 failed。测试：`review_rubric_moves_file_task_terms_to_judge`、`review_rubric_keeps_hard_items_for_code_tasks`、`write_fallback_names_the_file_without_claiming_missing_content`；集成测试要求 judge `votes>0`。
- N25：XML function 开标签漏 `>` 时在换行或 parameter 前截断；parameter 开标签同样容错。测试：`malformed_xml_function_open_without_gt_is_recovered`。
- N26：对比题按机制/连接、可靠性/一致性、性能与开销、场景逐轴展开，每维一行。测试：更新 `system_prompt_distinguishes_brevity_from_coverage`。
- N27：`BenchTempDir` 在 Drop 清理临时目录；启动清 24 小时前的 `fa-bench-*`，不动新目录。
- TUI：`draw` 接收 `&mut App`，渲染前把 `chat_scroll` 夹到 `max_scroll`，到顶后继续按 Up 不再积累虚拟滚动量。测试：中英 TUI 的 `chat_scroll_is_clamped_after_overscrolling_at_top`。

### 文档
- N28（Q7 粗体标题不被列表打分规则识别）记录为测试侧限制，不是产品改动。
- Windows x64 交叉编译 10 个 exe，PE 仅依赖系统 DLL；Wine 6.0.3 在临时 `bcryptprimitives` stub 下通过 `version` 与三次 `bench --json`，冷启动/RSS 不作 Windows 基线。
- Linux 全量 198 项、197 通过、1 ignored；release bench 三连跑内核 2,568,520 B，热启动 P50 1.221/1.198/1.174 ms，冷启动 P50 3.567/3.060/3.099 ms，gate 全绿。

## [1.2.1-rc.4.3] - 2026-11-25

### 修复
- N19：`serve` 用带重试的 `ipc::connect` 判断旧内核，干净安装根会先等满 3 秒管道 deadline 才启动，表现为固定约 3.18 s 冷启动。新增单次探测 `ipc::try_connect`；`serve`、`wait_ready`、冒烟探测与 `Server::stop` 改用它，正常客户端保留有界重试。测试：`try_connect_does_not_enter_the_retry_deadline`。
- N20：系统提示区分内容回答与文件创建。代码、文本、方案、译文默认写在答复里；只有用户明确要求保存、写入或创建文件时才调用文件工具。测试：`system_prompt_separates_content_output_from_file_creation`。
- N21：列举/比较类答复的第一层条目必须对应用户要求的对比轴；用户未给轴时覆盖机制、可靠性、性能与开销、适用场景，不要用同一维度的近义表述占位。测试：`system_prompt_distinguishes_brevity_from_coverage`。
- N22：明确的写文件任务把成功 `tool.fs.write` 的路径与内容作为带标签证据交给 review-gate，judge 检查文件而不是要求最终答复逐字复述；提示词同时禁止写成功后反复读取。测试：`review_file_evidence_is_only_for_explicit_write_tasks`、`review_answer_includes_written_file_content_before_final_answer`、`review_gate_validates_written_file_content_instead_of_answer_wording`。

### 新增
- `docs/model-test-reports/`：收录 9 份含可复核本地模型数据的 Windows 真机报告，附 rc.4.1 四类任务对照、rc.4.2 七题试卷、rc.2 → rc.4 review-gate 里程碑三份 CSV。

## [1.2.1-rc.4.2] - 2026-11-25

### 修复
- N15：上下文解析器恢复 Qwen/Ollama 的 XML 风格 function/parameter 工具调用（兼容 `=` 与 `name="..."`、参数值读到结束标签、缺失 function 结束标签也能解析）；Ollama provider 在流式 XML 报错时先从已累计文本抢救调用，不再直接终止整轮。测试：`xml_function_parameter_fallback`、`malformed_xml_function_without_close_is_recovered`。
- N16：Windows `ipc::connect` 在 3 秒预算内除 `ERROR_PIPE_BUSY` 外，也重试 `ERROR_FILE_NOT_FOUND(2)` 与 `ERROR_PIPE_NOT_CONNECTED(233)`；`bench` 显式创建 `rpc` 目录，并在 `wait_ready` 后对 `Client::connect` 做最多 10 秒有界重试。Linux 行为不变。
- N17：`bench --json` 不再把 `boot_gate_trusted` 放进 `gates`，而是与 `boot_gate_status`、`reliability` 一起移到 `advisory_gates`，低可信度主机不算发布门禁失败；`metrics` 保留该字段供诊断，人类可读输出单独打印建议项。
- N18：系统提示保留默认简洁，但用户要求列举、比较、穷举或说明全部要点时必须覆盖全部要点。测试：`system_prompt_distinguishes_brevity_from_coverage`。

## [1.2.1-rc.4.1] - 2026-11-25

### 修复
- N10：路径/文件型 `must_include` 使用 64 字上限（`PATH_MUST_INCLUDE_MAX_CHARS`），普通术语仍为 12 字；`workspace/notes.txt` 通过，超长路径按 64 字拒绝并提示改用 `pattern/regex`。
- N11：上下文解析器兜底识别正文里的 `tool_call <name> <json>`；`tool_fs_read`/`tool_fs_write`/`tool_fs_list`/`tool_shell` 归一化为标准工具名，review feedback 增加工具格式守卫。
- N12：search/news/fetch 结果除工作区 `artifact` 外，再写 `data/web-artifacts/<session>/` 审计副本，工具结果带 `audit_artifact`。
- N13：被接受的 rubric 落为 `data/review-artifacts/<session>/<ms>-rubric-final.json`，含规范化后的 `match`/`min_matches`；`review.guide` 新增 `final_artifact`。

### 变更
- 文档记录 N14：多任务长流程需 ≥7B exec 模型并多次取样；4B 结果只作观测，不作为回归门禁。

## [1.2.1-rc.4] - 2026-11-25

### 修复
- N5：`must_include` 支持 `match: "any" | "all" | "min"` 与 `min_matches`。多术语默认 `min`，要求覆盖 ceil(n/2) 项，不再强制全部字面命中；协议类硬约束可显式 `all`，列举类可用 `any`。hardcheck 报告 `{hits}/{total}`，语义正确但未覆盖全部字面的答案不再误杀。
- N6：主观判据（准确/准确无误/完整/正确/恰当/专业/规范/流畅/自然等）禁止使用 `must_not_include`；禁词若已出现在用户输入中，guide 也判不合格。避免“术语准确”禁掉 `软件/硬件/计算机`，把所有正确答案判死。N4 的 fixture 整条丢弃由 rc.3.5 原始回归测试继续覆盖。
- N7：bench 每次使用唯一临时目录（`fa-bench-<pid>-<random>`），Windows 复用 pid 或残留目录不再造成 `os error 2`；I/O 错误带操作与路径，`bench --json` 失败返回 `{"ok":false,"code":"artifact_missing"|"bench_failed","error":...}`。
- N8：同一条模型回复里的相同工具调用在执行前去重；每条 `tool.call` 增加 `attempt` 与 `call_index`，重试链里的重放可区分，不会静默执行两次。
- N9：`metrics.env.reliability` 非 `high` 时，启动门禁标为 `boot_gate_status:"advisory"`，保留 `boot_gate_raw`；`gates.boot_lt_*` 不再因整机内存压力单独判红。`reliability` 同步写入 gates 与 metrics。

### 新增
- 网络 search/news/fetch 结果以 pretty JSON 落到 `workspace/<项目>/.featheagent/search/<时间戳>-<工具>.json`；工具结果增加 `artifact` / `artifact_kind`，写失败只附 `persist_error`，不影响搜索本身。
- WebUI 附件：`+` 打开系统文件选择框，经 HTTP RPC（`file.attach`，base64，文件上限 8 MiB、请求体上限 12 MiB）上传，显示可移除的附件 chip 与内核返回路径。TUI 支持 `@<路径>` / `@"含空格路径>"`，先 `is_file` 校验再调用 `file.prepare`。
- 内核 `file.attach` / `file.prepare`：复核存在性；安装目录外的文件复制到 `workspace/default/uploads/`，返回模型应使用的路径；浏览器上传总是落到默认工作区。测试覆盖上传落默认工作区、外部文件复制、安装目录内原路径、附件解析（普通/引号/邮箱）、base64 往返、搜索 artifact JSON。

### 变更
- `GUIDE_SYSTEM` 说明 `match`/`min_matches`，禁止主观判据用 `must_not_include`。
- `bench` JSON 增加 `boot_gate_raw`、`boot_gate_trusted`、`boot_gate_status`、`reliability`。

## [1.2.1-rc.3.6] - 2026-11-25

### 修复
- N1：`must_include` 改为只收术语。`GUIDE_SYSTEM` 只允许专有名词、术语、文件名、命令或固定写法，2–12 字，禁止概念性描述、整句和连接词（`和/与/及/或/是/的/了`）。`MUST_INCLUDE_MAX_CHARS` 为 12；`must_not_include` 单独保留 20 字上限。`review.hardcheck` 按空白、`、`、`，`、`,`、`；`、`;`、`/` 拆词项，要求每个词项都出现；报告中的 `管理计算机硬件和资源` 会在 guide 阶段被拒，`管理计算机硬件 资源` 可在改写答案中通过。
- N2：`util::percentile` 内部排序。bench 只算一次 `boot_p50`/`boot_p90` 并复用，新增 `metrics.boot_percentiles_consistent` 与 `gates.metrics_consistent`；P50 ≤ P90 成为结构保证。
- N3：`run_turn` 对空响应最多重试 2 次（温度 0.70/0.75），对 Ollama 畸形工具调用最多重试 2 次并回注 JSON 格式纠错，均发 `model.retry`（`reason:"empty"` / `"tool_format"`）。插件崩溃仍重启一次；预算耗尽才发 `failed`。
- N4：`must_not_include` 命中 `REVIEW-FINAL`、`REVIEW-START`、`<...>`、`__FILL`、`{{`、`}}` 时整条丢弃并打 `review.warn`。

### 新增
- 全部五个顶层二进制的英文版：`featheragent-en`、`featheragent-tui-en`、`featheragent-webui-en`、`feather-plugin-en`、`feather-storage-en`。WebUI 用 `-en` 后缀选择英文资源；kernel/TUI 使用独立英文源码 target；plugin/storage 是纯协议别名。
- Windows zip 含 10 个 exe（5 中文 + 5 `-en`）；Linux core 含 8 个二进制（4 中文 + 4 `-en`）；WebUI 包同时含中英两版。
- 测试：`percentile_works_on_unsorted_samples_and_p50_le_p90`、`tool_format_error_detector_covers_ollama_and_json_errors`、`review_rubric_rejects_conceptual_must_include_with_connective`、`review_rubric_rejects_one_char_must_include`、`review_hardcheck_matches_include_terms_across_wording`、`review_rubric_drops_fixture_must_not_include_item`、`english_suffix_selects_english_assets`。

### 变更
- Linux 自动化：**162 次测试执行，161 通过、1 ignored**；英文 worker/WebUI 别名 target 重复执行 26 个 plugin 与 4 个 WebUI 用例。`fmt` 与 `clippy -D warnings` 通过。
- bench：热启动 P50 1.142/1.167/1.163 ms，P90 1.761/1.190/1.170 ms；冷启动 3.722/2.324/2.262 ms；门禁全绿。
- 没有改动 wire 消息、契约、provider、storage、tool 或插件设置；bench 字段为增量。

## [1.2.1-rc.3.5] - 2026-11-25

### 修复
- M1：guide 不再把占位符示例写成判据。`GUIDE_SYSTEM` 改用 `__FILL_*__` 形态，禁止在判据里讨论占位符/模板；`review.parse_rubric` 丢弃自我指涉的占位条目、保留真实条目。a2 的 rubric 现在保留 c1/c3、丢弃 c2、打 `review.warn`，整轮不再 degraded。
- M1：`must_include` 接受字符串数组（`["SYN","ACK"]`），hardcheck 逐项要求全部出现，不再静默退化为描述型。
- M3：主观描述判据（简洁、清晰、排版好看等）标记为 `advisory`，不进入 hardcheck 与 judge，并打 `review.warn`。内核在重试用尽后仍没有可执行判据时直接失败，不再因为条目非空就放行。
- M4：S3 t6 提示词明确要求先调用一次 `tool.fs.read`，禁止不调工具凭猜测回答。

### 新增
- `review.parse_rubric` 输出 `advisory` id 列表；`review.warn` 报告被丢弃的占位条目与主观降级。
- 测试：占位元判据清洗、主观 advisory、数组 `must_include`，以及集成 `review_gate_drops_placeholder_meta_item_and_approves`、`review_gate_subjective_criterion_is_advisory`。

### 变更
- `GUIDE_SYSTEM` 规则 8/9：示例只示范字段名；禁止主观判据，改用可计数检查。
- 没有改动 wire 消息、契约、provider、storage、tool 或插件设置。

## [1.2.1-rc.3.4] - 2026-11-25

### 修复
- L1：guide 不再抄 JSON schema。`GUIDE_SYSTEM` 的示例值全部改成尖括号占位符（`<...>`），删掉 mock 专用 `REVIEW-FINAL`；`review.parse_rubric` 拒绝 `criterion` / `check` / `must_include` / `must_not_include` 里的占位文本。`must_include` 为 `REVIEW-FINAL` / `REVIEW-START` 且未出现在用户输入时直接拒绝。该检查只限这两个 mock 标记，避免误杀 `HTTP`、`JSON` 这类任务术语。
- L2：`must_include` 缺失时的 evidence 不再回显字面短语，改成“答案未覆盖判据「…」要求的关键内容”，exec 没有可粘贴的标签。
- rubric 解析现在拿到用户输入，用于区分夹具令牌与真实任务。

### 新增
- review 生成物落盘：guide 与 judge 每次模型调用都在解析前写入 `<data>/review-artifacts/<session>/`。guide 文件为 `<unix-ms>-guide-a<attempt>.json`，judge 文件为 `<unix-ms>-judge-v<vote>-a<attempt>.json`，保留原始输出与解析错误。
- `review.guide` 增加 `artifact`；`review.verdict` 增加 `attempts`、`votes`、`artifacts`。落盘是 best-effort，不会因磁盘错误拖垮门禁。
- 测试：`review_rubric_rejects_schema_placeholders`、`review_hardcheck_does_not_leak_must_include_literal`，以及 `review_gate_three_roles_retry_and_approve` 的端到端生成物断言。

### 变更
- 判据校验更严：抄 schema 的 guide 会被拒绝并重试，不再生成永远无法满足的判据。
- 插件设置未变，`plugins/review-gate/plugin.json` 仍是 14 个字段；生成物始终记录。

## [1.2.1-rc.3.3] - 2026-11-25

### 修复
- K1：判据先分流。`must_include` / `must_not_include` / `min_points` / `pattern` / `regex` 交给 `review.hardcheck`；只有描述型判据进入 `review.judge`，负向判据通过时不再需要“答案原文”这种不可能存在的证据。
- K2：judge 审批缺项时，把完整验收项 id 清单回注并在同一票内补投；漏一项不再整轮 degraded。
- K3：`bench --json` 增加 `boot_ms_p90` 与 `boot_sample_count`；门禁仍看中位，但尾部可见。
- K4：`metrics.env` 在 Linux 也输出 `mem_load_pct`，并增加 `reliability`；内存 ≥85% 时附 `note`，门禁不转红，只标注读数可信度。

### 新增
- 判据键 `regex` + `min_matches`（1–50，正则最长 200 字符），guide 阶段即编译校验。
- `hardcheck.pattern_engine`（默认 `glob` / `regex`）、`criteria_routing`（默认 `auto` / `hardcheck_only` / `judge_only`）。
- 串行 `judge.votes`（1 或 3）、`majority` 聚合、`early_stop_on_agreement`；温度 base / +0.05 / +0.10 封顶 0.2；峰值模型并发仍为 1。

### 变更
- `plugins/review-gate/plugin.json` 声明 14 个设置字段（原 9 个），TUI 与 WebUI 通用渲染。
- `min_points` 的 `review.warn` 文本改为“按软判据接受…”，`hardcheck_only` 会提示有多少条描述型判据未评估。

## [1.2.1-rc.3.2] - 2026-11-24

### 修复
- J1：`min_points` 不再单独判死一轮。`count_points` 增加中文分号分段、加粗小标题与空行段落识别（取最大值，上限 12）；计数不足改为软判据，引擎打 `review.warn` 并把机器计数提示交给 judge，由 judge 按答案原文裁决。
- J2：review feedback 不再用裸 `c1:` 前缀（exec 会把它抄进答案）。改为 `〔1〕`/`〔2〕` 自然序号，并明确禁止把序号或 `c1/c2` 编号写进答案。
- `featheragent-tui version` / `featheragent-webui version` 改为打印 `FeatherAgent TUI` / `FeatherAgent WebUI`（二进制名不变）。

### 新增
- `bench --json` 的 `metrics.env` 增加环境指纹：采集时间、CPU 数、负载、总/可用内存与 uptime（平台支持时），人类可读输出加一行。门禁阈值不变。

## [1.2.1-rc.3.1] - 2026-11-24

### 修复
- 真实 7B guide 下的 review-gate：guide prompt 给出明确的 JSON schema 与 few-shot；`review.guide` 支持 `feedback`，`review_prepare` 把上一轮 `review.parse_rubric` 的错误回注进重试，而不是原样重发（最多 3 次）。
- `review.parse_rubric` 接受 1–5 条，只带描述的条目标为 `judge_only` 保留，整份清单只需一条硬判据；始终是软判据时以 judge-only 评估并打 `review.warn`，不再整轮 `degraded`。
- `tools/s3_batch.py` t6 现在只允许一次工具调用，并禁止 `tool.shell` / `tool.ping`。

### 新增
- `EventKind::ReviewWarn`（`review.warn`，编号 29），用于判据质量告警与被拒重试。
- `SettingField.options_source` / `allow_custom`；`plugins/review-gate/plugin.json` 把模型字段声明为 `models`，TUI 与 WebUI 通过 `provider.models` 渲染真下拉（仍可手动输入）。
- `tool.confirm_request` 带 `arguments` 与人读 `preview`；TUI 新增权限弹窗，WebUI 弹窗展示完整命令。

### 变更
- TUI 插件页：任意值字段 Enter 即保存整张表单（启停行先翻转再保存）。
- 用户可见产品名统一为 **FeatherAgent**（二进制、子命令、包名与路径不变）。

## [1.2.1-rc.3] - 2026-11-24

### 新增

- `manifest.settings` 插件契约：`root`、`title`、`description` 与 `fields[]`；字段类型限定 `bool|int|number|string|enum`，支持 `default`、区间、选项与可选绝对 `path`。
- `plugin.settings` RPC：返回声明 schema、当前值与默认值；未声明设置项的插件返回业务错误。
- `Config.plugin_settings` 命名空间，作为未声明 `root` 插件的兜底。

### 变更

- 插件设置不再塞进通用设置页：TUI 与 WebUI 都按声明通用渲染任意插件的表单，装新插件不需要改前端。
- 登记表持久化每个插件的设置 schema，并自动补齐本版之前登记的旧条目。
- `config.set` 按声明校验（类型、区间、枚举），首次写入自动补父级；`plugin_settings.<name>` 下未声明的键被拒绝。
- 插件侧边栏与详情标题以插件名为主标签，类别只作次要信息。

### 修复

- H1：判据限制为 2–5 条短语（`must_include` ≤20 字、拒绝整句），且至少一条 `must_include`、至少一条非子串判据（`must_not_include`、`min_points`、`pattern`）；judge 前的硬校验覆盖四类。
- H2：judge 的 evidence 与总结必须是答案中连续 ≥10 字原文；抄提示词与占位词一律拒绝。
- H3：未知工具名返回 `未知工具 … 可用工具：…` 的正常工具错误，不再原样转发成 RPC 失败。
- H4：S3 的 t5 强制调用 `tool.fs.read`、t6 强制只调用一次，S3 口径可复现。

## [1.2.1-rc.2.1] - 2026-11-24

### 修复

- R-01：原生 Ollama 模型发现改走 `/api/tags`，不再请求会 404 的 `/models`；TUI 与 WebUI 的 `provider-ollama` 模型列表恢复。
- R-02：TUI `/settings` 可编辑并保存 review-gate 的启用、轮次、guide/exec/judge 模型与温度；校验温度区间，19 字段表单支持滚动。
- R-03：review rubric 强制字面 `must_include`；内核在 judge 前硬校验，修订反馈带上 `criterion` 与 `check`。
- R-04：judge evidence 少于 10 字视为失败并重试 judge 一次；guide rubric 失败重试一次；review 空响应自动重试一次。
- R-05：`chat.finished.status` 按评审结果返回 `ok`/`unapproved`/`degraded`；新增 `review.status` 与规范化 verdict `items`。

### 变更

- TUI 与 WebUI 的 review 提示建议 guide/judge 使用 ≥7B 模型，4B 可能无法产出合规字面 rubric。

## [1.2.1-rc.2] - 2026-11-24

### 修复

- P0：provider API Key 为空时，空的 Authorization 在 `Content-Length` 之前留下空行，HTTP 头被提前终止，Ollama/OpenAI 兼容请求表现为空 body（`400 missing request body`）。现改为逐行追加头，`Content-Length` 与 body 始终存在。
- P1：review-gate 三角色温度区间与 `enabled` 无关，越界值在写入 `config.json` 前拒绝。
- P2：`serve` 打印最终生效的 model/provider/base URL 及来源；CLI 与 config 冲突时显式 warning。
- embedding 请求使用当前模型，不再回落到 provider 默认模型。

### 新增

- 空 Key 请求体回归测试（`/api/chat`、`/api/embed`）、原始头/body 布局测试、review-gate 区间拒绝测试。

## [1.2.1-rc.1] - 2026-11-24

### Added

- 英文成为文档主语言；中文版统一使用 `.zh-CN.md` 后缀，根目录与核心文档均保留中英双版。
- TUI 与 WebUI 只保留中文界面；已移除 `--lang` 切换与英文界面资源，内核保持语言无关。
- Windows 包同时包含英文测试指南与中文版 `START-HERE.zh-CN.md`。
- Windows 可执行文件图标与 WebUI favicon 改用透明底羽毛图标；`logo-tile.png` 仍作为 Apple touch icon 的不透明底图。
- `review-gate` 循环插件：guide → exec → judge，结构化验收清单/审批结果、必须有 evidence 才通过、1–5 轮修订、review 事件与 WebUI 设置。
- 本地 provider 插件：`provider-llamacpp`、`provider-ninfer`、`provider-lmstudio`、`provider-vllm` 与通用 `provider-openai`，均走 OpenAI 兼容协议。
- 所有 provider 支持可选 Bearer Key，含 Ollama 原生与 OpenAI 兼容路径；TUI/WebUI 可填写，仅存本机。

### Fixed

- R1 启动门禁：热启动改为同一安装根、预热 1 次丢弃、再取 5 次中位；真实 spawn 冷启动单独计量并单设门禁。Windows 插件登记/杀毒尖峰不再干扰 100 ms 热启动线。

### Changed

- `README.md`、`CHANGELOG.md`、`CONTRIBUTING.md`、`SECURITY.md`、`CODE_OF_CONDUCT.md`、架构设计、验证报告、Wire Format、Windows 测试指南与 v1.2.0-rc.1 变更说明默认英文。
- 历史 Windows 复测记录仅保留中文，文件名统一加 `.zh-CN.md`。
- bench 字段新增 `boot_mode`、`boot_samples_ms`、`boot_warmup_discarded_ms`、`cold_spawn_mode`、`cold_spawn_ms_p50` 与分平台冷启动门禁；`spawn_to_socket_ms_min` 保留为兼容别名。

## [1.2.0-rc.1] - 2026-11-23

### Added

- MIT `LICENSE`，以及 `SECURITY.md`、`CONTRIBUTING.md`、`CODE_OF_CONDUCT.md`、本 CHANGELOG。
- 本地 CI `scripts/ci.sh`：Linux fmt/clippy/test/release build；`--with-windows` 追加 Windows x64 交叉编译并检查 `.rsrc` 与 MinGW 运行时 DLL。
- `.github/CODEOWNERS` 与 Pull Request 模板。
- `scripts/version-check.sh`：校验 Cargo、锁文件、5 个插件 manifest 与 WebUI 版本占位一致。
- `scripts/build-linux.sh`：Linux 版本门禁 + `--locked` 构建入口。
- `docs/v1.2.0-rc.1-变更说明.zh-CN.md`；README 增加平台支持矩阵与“已实测 / 未实测”表。

### Fixed

- `web.search` / `web.news` 的 `count` 接受数字字符串（`"5"`）并在内核转发前归一化为整数；schema 仍为 integer。
- TUI 插件页标题不再硬编码 `v1.1.8`，改为 `CARGO_PKG_VERSION`。
- WebUI 顶栏版本改为启动时注入 `{{VERSION}}`，不再硬编码。
- 本地搜索 User-Agent 版本改为 `CARGO_PKG_VERSION`。
- `featheragent --version` 不再误启动内核；TUI/WebUI 新增 `--version`/`--help`。
- 修复 clippy 报告的 16 项代码问题（类型别名、范围循环、`&Path`、`checked_div`、排序等），`cargo clippy -D warnings` 清零。
- 集成测试版本断言改为当前包版本，避免发版时误判。

### Changed

- Windows PE 资源新增 `FileVersion` / `ProductVersion` / MIT 版权，CompanyName 改为 `FeatherAgent contributors`。
- Linux 与 Windows 打包都带 `LICENSE`；Windows 另带 SECURITY/CONTRIBUTING/CODE_OF_CONDUCT/CHANGELOG。
- `Cargo.toml` 增加 `license = "MIT"`、`repository`、`readme`、`authors`、`rust-version = "1.80"`。

### Security

- 扫描全部提交历史与工作区：未发现 `sk-*`、`ghp_*`、AKIA、私钥、`.env` 或大体积二进制泄漏；仅 `target/`、`dist/`、`__pycache__/` 被忽略。

### Verification

- Linux：**86/86** 自动化测试通过（57 lib + 2 plugin + 2 kernel + 3 TUI + 3 WebUI + 2 golden + 17 integration），1 个需公网用例单独通过。
- `cargo fmt --check`、`cargo clippy --all-targets --locked -- -D warnings` 通过。
- Windows x64 交叉编译 5 个 exe；具体哈希见 Release 资产与 manifest。

## [1.1.9] - 2026-11-23

### Fixed

- 项目工作区路径双重嵌套：`tool_base()` 只去掉一层当前项目名前缀。
- `/stop` 提前停止误报 `failed`：取消态发 `finish=interrupted` 空消息，状态统一为 `stopped/cancelled`。
- WebSocket 空闲 15 秒后页面卡 `running`：事件线程超时后继续转发，仅断开连接才退出。
- `tools/s3_batch.py` 适配项目级工作区、唯一 session 与写盘断言。

### Added

- TUI/WebUI 助手消息 Markdown 渲染（标题、列表、引用、代码块、行内样式、链接）。
- WebUI/Windows 羽毛图标（favicon、logo、5 个 exe 资源段）。
- 零 Key 本地联网搜索（必应/DuckDuckGo/Wikipedia/HN/GitHub + 新闻/抓取/联想/状态），默认 `search_provider=local`。
