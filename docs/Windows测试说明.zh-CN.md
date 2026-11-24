# FeatherAgent 1.2.1-rc.3.2 · Windows x64 开源候选版测试说明

> 被测包：`FeatherAgent-1.2.1-rc.3.2-windows-x64.zip`
> 基线：v1.1.9 真机复测报告（Q1/Q2）
> 说明：v1.2.1-rc.3.2 修 rc.3.1 复测的 J1–J3：`min_points` 改为软判据交 judge 复核（同时放宽中文要点计数并打 `review.warn`），review feedback 不再用会被抄进答案的裸 `c1:`，`bench --json` 增加 `metrics.env` 环境指纹。它也延续 v1.2.1-rc.3.1 的修复：rc.3 真机暴露的“真实 7B guide 全量 degraded”：guide 现在拿到明确的 JSON schema，校验错误会回注进重试，只有描述项的判据改为 judge-only 评估并打 `review.warn`，不再整轮降级。插件设置新增由 `provider.models` 填充的模型下拉（仍可手动输入），TUI 插件页任意字段 Enter 保存，两端权限申请显示完整命令，产品显示名统一为 **FeatherAgent**（二进制与路径不变）。rc.3 把插件设置改成声明式——插件在 `plugin.json` 里写清 schema，内核通过 `plugin.settings` 原样转述，TUI 与 WebUI 通用渲染，装新插件不用改前端。同时修掉 rc.2.1 复测的 H1–H4：判据为 2–5 条短语（`must_include` ≤20 字、至少一条非子串判据）、judge 前硬校验覆盖字面/禁词/要点数/glob、judge evidence 必须是答案中连续 ≥10 字原文、畸形工具名返回“未知工具 + 可用清单”、S3 t5/t6 强制只调用一次工具。**Windows 真机结论以你的复测为准**；Wine 只验证基本启动，不证明命名管道并发、ConPTY、代理或真实杀掉时序。

## 1. 解压与启动

解压到有写权限的目录（不要放 `Program Files`）：

```powershell
D:\FeatherAgent-1.2.1-rc.3.2\
```

首次启动可能 1–2 分钟：Windows 首次执行每个插件二进制会触发杀毒扫描，内核会打印：

```text
[init] 登记内置插件 storage（首次启动可能被杀毒软件扫描，耗时较长）
[init] 登记内置插件 tool（首次启动可能被杀毒软件扫描，耗时较长）
...
```

看到 `serve ready: 1.2.1-rc.3.2` 后再操作。

```powershell
cd D:\FeatherAgent-1.2.1-rc.3.2
.\featheragent.exe help
.\featheragent.exe serve --root . --provider provider-ollama --model qwen3.5:4b
```

另一个终端：

```powershell
.\featheragent-tui.exe --root .
.\featheragent-webui.exe --root . --app
.\featheragent.exe chat --root . --text "hello"
```

## 2. S1 项目级工作区

`workspace_root` 现在是**项目容器**，容器下的项目目录才是工作区：

```powershell
# 用外置容器启动（也可在 WebUI/TUI 设置页把 workspace_root 指过去，重启 serve）
.\featheragent.exe serve --root . --workspace D:\Projects\fa-ws --provider provider-ollama --model qwen3.5:4b
```

预期：

```text
D:\Projects\fa-ws\
├── default\
│   ├── .featheagent\worklog.md
│   ├── .featheagent\memory.md
│   └── .featheagent\README.md
└── proj-a\                  ← 新建项目/会话后
    └── .featheagent\
```

检查每个项目都有自己的 `.featheagent`，而不是只在容器根上一份。让模型写 `proj-a/hello.txt`，文件应落在 `D:\Projects\fa-ws\proj-a\`，不串到 `default`。

TUI：

```text
/workspace proj-a       # 创建项目
/workspaces             # 列出容器内项目及 .featheagent 状态
/new s1 proj-a          # 会话绑定 proj-a
```

WebUI：顶栏项目下拉 + “＋项目”，新建会话时选择项目；恢复会话会显示项目名。

## 3. S2 停止、引导、压缩

### 紧急停止并保存上下文

发送一个长任务（或 `SPIN`），在生成中：

- TUI 按 `/stop`，或 WebUI 点“停止并保存”。
- 预期：`chat.finished(status=stopped)`；已生成的部分文本仍在会话里；`worklog.md` 保留；下一条消息可以直接发送。

### 回合内插入引导

生成中直接输入新文本（TUI 直接回车；WebUI 输入框仍可用）：

- 预期：事件日志出现 `user.message(source=steer, during_turn=true)`；当前生成被中断，模型在下一轮看到引导并调整方向。
- 若当前没有运行回合，`chat.inject` 会提示改用 `chat.send`。

### 压缩上下文

至少完成两个回合后：

- TUI `/compact`；WebUI 点“压缩上下文”。
- 预期：`context.compacted` 事件写入日志，摘要覆盖旧事件；模型视图不再包含旧原文，但旧原文仍可从日志回放；重启后会话仍可恢复。

## 4. S3 本地联网搜索（默认，零 Key）

设置页“搜索 Provider”默认是 `local`，不需要任何 API Key。预期：

- 让模型“搜索今天的柳州天气”，出现 `tool.call` → `web.search` → `tool.result`，结果含 `title/url/snippet/source/rank` 与 `untrusted:true`。
- 中文网络下走必应；技术问题会追加 Hacker News；`engine=all` 才加入 GitHub/Wikipedia。
- `web.news` 读取 Google News RSS，国内不可达时回落到中新网 RSS（`source=chinanews`，`query_matched=false` 表示只返回最新要闻）。
- `web.fetch` 抓公开网页；`localhost` 与私网会被拒绝。
- `web.suggest` 返回联想词；`web.status` 返回 `provider=local`、引擎列表、`api_key_required=false`。

**DeepSeek（可选）**：设置页切到 `deepseek`，填 `DEEPSEEK_API_KEY` 后才启用；按成功请求计费（5 元/1,000 次）。不想联网就把 `search_provider` 设为 `none`。

## 5. Markdown 与图标

- TUI：发送含 `# 标题`、`- 列表`、`1. 有序列表`、`> 引用`、代码块、`**粗体**`、`*斜体*`、`[链接](https://…)` 的消息，助手回复应去掉 Markdown 标记并保留结构。
- WebUI：同样的消息应渲染为标题、列表、引用、代码块和可点击链接；助手消息不再显示原始 `**` 或三重反引号。
- 任务栏/程序图标：资源管理器中 5 个 exe 应显示青色羽毛图标；WebUI 左上角与浏览器标签页 favicon 也应显示羽毛。

## 6. 回归命令

```powershell
# NEW-K：内核运行时跑 bench，必须退出码 2
.\featheragent.exe bench --root . --json
echo "bench exit=$LASTEXITCODE"

# 门禁（内核停止时）
.\featheragent.exe bench --root . --json

# 工具闭环 + 写盘
.\featheragent.exe chat --root . --session s3 --text "写文件" --allow tool.fs.write
```

预期 `bench` 冲突时打印“检测到内核正在运行（pid N）”且 `exit=2`，不再被脚本吞成 0。

R1 核对（内核停止、Release 包）：

- `metrics.boot_mode` 为 `warm_in_process_same_install_root`；`boot_samples_ms` 有 5 个数；`boot_warmup_discarded_ms` 存在。
- `gates.boot_lt_100ms` 检查的是**热启动**。
- `metrics.cold_spawn_ms_p50` 存在，`gates.cold_spawn_lt_250ms` 为 true；冷启动是真实 `serve` 进程，不是进程内数字。
- 连续跑 3 次 bench，热启动应稳定低于 100 ms，不再出现旧的 130–175 ms 尖峰。

Q1/Q2 小项：

- `/stop` 后 TUI 状态栏应显示 `cancelled`（源码与二进制均已核对；`findstr /C:"cancelled" .\featheragent-tui.exe`）。
- 模型把 `count` 写成字符串 `"5"` 时，`web.search` 仍返回 5 条；`tool.result.count` 与请求一致。
- `web.search` 的 schema 声明是 `integer`，字符串容错只发生在内核转发层，不改变对外契约。

v1.2.1-rc.2 回归核对：

1. 真实 Ollama + `provider-ollama`，API Key 留空，发送普通消息。不得再出现 `400 missing request body`。查看服务/代理日志：`POST /api/chat` 的 `Content-Length` 非 0，JSON body 含 `"model"` 与 `"messages"`；`/api/embed` 也必须有 body。
2. 在 review-gate **关闭**时，用 `config.set` 设 `review_gate.judge.temperature=0.8`，必须被拒并提示 `超出允许区间 [0.0, 0.2]`，`config.json` 保留旧值；guide 0.9、exec 1.9 同样应被拒。
3. 用与 `config.json` 不同的 `--model` 启动 `serve`，banner 必须打印 `effective model=<CLI 值> ... (model_source=cli ...)`，并出现 `warn: --model 覆盖 config.json ...`；`config.get` 与 `review.guide` 事件 payload 都必须返回 CLI 值。
4. 再启用 review-gate 跑一次 `REVIEW_START`，guide/verdict/feedback/finished 事件序列应保持不变。

v1.2.1-rc.2.1 回归核对：

1. 原生 Ollama 下打开 TUI 与 WebUI 设置页，模型列表必须从 `GET /api/tags` 拉全；不得再因 `GET /models` 404 而为空。
2. TUI `/settings` 中确认 9 个 review 字段都可见可达：启用、最大轮次、guide 模型/温度、exec 模型/温度、judge 模型/温度。`Ctrl+S` 保存后重启 TUI，值必须从 `config.json` 回读；缩小终端确认当前字段会滚动到可视区。
3. 用真实 7B guide/judge 启用 review-gate，发 `REVIEW_START`。`review.guide` 必须含 `must_include`；`review.verdict.items[]` 必须带 `criterion` 与 `check`；通过项的 evidence 必须引用答案原文至少 10 字。只有 approved 时 `chat.finished.status` 才能是 `ok`。
4. 构造未通过评审（或设 `review_gate.max_rounds=1` 并用无法满足 rubric 的模型）。`chat.finished.status` 必须是 `unapproved`；guide/judge 连续两次失败时是 `degraded`，不得是 `ok`。
5. 全 4B review：若模型无法产出字面 rubric，回合必须 `degraded`，不得静默通过。

v1.2.1-rc.3 回归核对：

1. **两端插件自述设置。** TUI `/settings` 与 WebUI 插件页：侧边栏每个插件都以插件名出现；`review-gate` 显示 9 个声明字段（启用、最大轮次、Guide 模型/温度、Exec 模型/温度、Judge 模型/温度、轮次用尽），`search` 显示 4 个；未声明设置的插件显示“未声明设置项”+ 启用开关，而不是空表单。
2. 在 TUI（`Ctrl+S`）与 WebUI（保存插件设置）各把 `max_rounds` 改成 3，读 `data/config.json`：两边都必须落到 `review_gate.max_rounds = 3`。常规设置页不得再出现任何 review-gate 或联网搜索字段。
3. 关闭设置页后在两端各发一条消息：仍要收到 `model.message` 与 `chat.finished(ok)`。
4. 用真实 7B guide/judge 发一个长答案任务：判据必须是 2–5 条短语（不能整句），且至少一条为非子串判据；违禁词、要点数、glob 不满足时 `review.hardcheck` 必须给出确定性失败。
5. 正向核对 judge evidence：`review.verdict.items[].evidence` 必须抄自答案；抄提示词要触发 judge 重试，两次失败才 `degraded`。
6. 工具名纠错：让模型用一个不存在的工具（或喂 `TOOL:{"name":"tool.fs"}`），`tool.error` 必须返回 `未知工具 tool.fs。可用工具：…`，且回合并未让插件熔断，而是继续对话。
7. S3 复现性：跑 `tools/s3_batch.py`（或手工同题）。t5 必须先调一次读工具再回答，t6 必须恰好调一次后报告失败。

v1.2.1-rc.3.1 回归核对：

1. **真实 7B guide 跑通门禁。** 打开 review-gate 发一条普通任务：`review.guide` 的判据必须至少一条带 `must_include` / `must_not_include` / `min_points` / `pattern`；只有 `criterion`/`check` 不算判据。若 guide 仍只给描述，回合必须出现 `review.warn`（“没有硬判据”）并继续交给 judge，而不是直接 `degraded`。
2. **重试带上错误。** 第一次判据被拒时，必须看到 `review.warn` 写明“第 1 次判据不合格：…”，且第二次 guide 的回答与第一次不同（内核回注了错误，不再原样重发）。
3. **模型下拉。** 在 WebUI 与 TUI 打开 review-gate 页：`guide.model`、`exec.model`、`judge.model` 必须是由本地模型服务（`provider.models` / Ollama `/api/tags`）填充的下拉，并带“（跟随当前模型）”选项；选中后保存，读 `config.json` 核对。再手动键入一个服务未返回的模型名，确认也能保存（自定义入口必须保留）。
4. **任意字段 Enter 保存（TUI）。** 打开插件页，把光标移到某个值字段（不是保存行），按 Enter，退出再进：值必须已经落盘。启停行按 Enter 要翻转并保存。
5. **权限申请带完整命令。** 触发 `tool.shell`（让它执行 `echo hi`）。TUI 弹窗与 WebUI 弹窗都必须显示完整命令 `echo hi`，不能只有工具名和风险等级；按 `n` / Esc 拒绝时不得执行。
6. **S3 t6 只调一次。** 跑 `tools/s3_batch.py`：t6 必须只调一次读工具，随后解释失败，不得再补 `tool.shell` / `tool.ping`。
7. **H3 可构造入口。** 不需要模型配合：用 `provider-mock` 执行 `featheragent chat --text 'TOOL:{"name":"tool.fs","arguments":{"path":"a.txt"}}'`，应得到含 `未知工具 tool.fs` 与可用清单的 `tool.error`。

v1.2.1-rc.3.2 回归核对：

1. **J1：`min_points` 变软。** 用 7B guide 让 exec 输出结构化答案，把三个要点写成一段里用 `；` 分隔，或用 `**加粗小标题**`。回合不得再以“只有 1 个要点”硬拒绝；若机器计数仍不足，必须出现 `review.warn`“min_points 机器计数不足，交 judge 复核”，由 judge 按答案实际内容裁决。
2. **J2：答案里不再出现 rubric 编号。** 构造一次失败（例如判据 `min_points:2`、答案只有一行）：`review.feedback.text` 必须用 `〔1〕` 序号，不能出现 `- c1:`；修订后的答案不得带 `c1/c2/c3` 小标题。
3. **J3：bench 环境指纹。** `featheragent.exe bench --json` 必须含 `metrics.env`：`cpu_count`、`mem_total_kb`、`mem_available_kb`、`mem_load_pct`、`uptime_s`；人类可读输出要有“环境指纹”一行。每次记 bench 数字时一并记录该指纹。
4. **显示名。** `featheragent.exe version`、`featheragent-tui.exe version`、`featheragent-webui.exe version` 都必须打印 `FeatherAgent …`（含 TUI/WebUI），二进制名仍保持小写。

v1.2.1 追加核对：

1. 设置页应出现 `provider-llamacpp`、`provider-ninfer`、`provider-lmstudio`、`provider-vllm`、`provider-openai`；切换 provider 时，如果 Base URL 为空或仍是某个默认值，应自动填对应默认端口。
2. 启动本地 OpenAI 兼容服务（llama.cpp `--server` 或 NInfer），选择对应 provider，刷新模型列表并对话，确认回复来自本地服务。
3. 填写可选 `API Key`，确认本地服务/代理收到 `Authorization: Bearer <key>`；清空后确认不再发送该头。
4. 用 `provider-mock` 启用“三段校验 review-gate”，发送 `REVIEW_START`。预期出现 `review.guide`、至少一次失败的 `review.verdict`、`review.feedback`、第二次通过的 `review.verdict`，最终答案 `REVIEW-FINAL`，且 `chat.finished.review.approved=true`。
5. 把 guide/exec/judge 温度设到 0.2–0.5 / 0.6–1.5 / 0.0–0.2 之外，保存必须被拒绝。

## 7. 已知边界

- N2 只是增加 `[init]` 进度，**没有消除**首次杀毒扫描耗时。
- 旧版 `<workspace_root>/<session>/` 空目录不会自动清理；确认无内容后手动删。
- WebUI Markdown/图标已在 Linux 真实浏览器验证；TUI Markdown 只做单测，仍需 PTY 真机确认。
- v1.2.1-rc.3.1 与 v1.2.1-rc.3.2 均未跑 Wine；Wine 通过也不等于 Windows 通过。
- 仍需复测：高 DPI/中文渲染、真实 Ollama 7B/4B 指标、代理环境、网络盘/同步盘、24h 浸泡。

## 8. 产物校验

```powershell
Get-FileHash .\FeatherAgent-1.2.1-rc.3.2-windows-x64.zip -Algorithm SHA256
```

以 Release 资产与 manifest 的字节数/哈希为准；包内 5 个 exe 只依赖 Windows 系统 DLL，根目录含 `LICENSE`、`SECURITY.md`、`CONTRIBUTING.md`、`CHANGELOG.md`。
