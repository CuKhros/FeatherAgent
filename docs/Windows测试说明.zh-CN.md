# FeatherAgent 1.2.1-rc.1 · Windows x64 开源候选版测试说明

> 被测包：`FeatherAgent-1.2.1-rc.1-windows-x64.zip`
> 基线：v1.1.9 真机复测报告（Q1/Q2）
> 说明：v1.2.1-rc.1 关闭 R1：热启动与冷启动 spawn 分开计量；新增 `review-gate` 三角色插件、5 个 OpenAI 兼容本地 provider，以及 Ollama 等 provider 的可选 Bearer API Key。**Windows 真机结论以你的复测为准**；Wine 只验证基本启动，不证明命名管道并发、ConPTY、代理或真实杀掉时序。

## 1. 解压与启动

解压到有写权限的目录（不要放 `Program Files`）：

```powershell
D:\FeatherAgent-1.2.1-rc.1\
```

首次启动可能 1–2 分钟：Windows 首次执行每个插件二进制会触发杀毒扫描，内核会打印：

```text
[init] 登记内置插件 storage（首次启动可能被杀毒软件扫描，耗时较长）
[init] 登记内置插件 tool（首次启动可能被杀毒软件扫描，耗时较长）
...
```

看到 `serve ready: 1.2.1-rc.1` 后再操作。

```powershell
cd D:\FeatherAgent-1.2.1-rc.1
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
- v1.2.1-rc.1 未跑 Wine；Wine 通过也不等于 Windows 通过。
- 仍需复测：高 DPI/中文渲染、真实 Ollama 7B/4B 指标、代理环境、网络盘/同步盘、24h 浸泡。

## 8. 产物校验

```powershell
Get-FileHash .\FeatherAgent-1.2.1-rc.1-windows-x64.zip -Algorithm SHA256
```

以 Release 资产与 manifest 的字节数/哈希为准；包内 5 个 exe 只依赖 Windows 系统 DLL，根目录含 `LICENSE`、`SECURITY.md`、`CONTRIBUTING.md`、`CHANGELOG.md`。
