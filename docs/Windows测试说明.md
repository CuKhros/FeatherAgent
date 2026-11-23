# FeatherAgent 1.1.8 · Windows x64 修复版测试说明

> 被测包：`FeatherAgent-1.1.8-windows-x64.zip`
> 基线：v1.1.7 真机复测报告（N1/N2/N3/N4/N6/N7）
> 说明：v1.1.8 新增项目级工作区、TUI 设置侧栏、紧急停止/引导/压缩、联网搜索。**Windows 真机结论以你的复测为准**；Wine 只验证基本启动，不证明命名管道并发、ConPTY、代理或真实杀掉时序。

## 1. 解压与启动

解压到有写权限的目录（不要放 `Program Files`）：

```powershell
D:\FeatherAgent-1.1.8\
```

首次启动可能 1–2 分钟：Windows 首次执行每个插件二进制会触发杀毒扫描，内核会打印：

```text
[init] 登记内置插件 storage（首次启动可能被杀毒软件扫描，耗时较长）
[init] 登记内置插件 tool（首次启动可能被杀毒软件扫描，耗时较长）
...
```

看到 `serve ready: 1.1.8` 后再操作。

```powershell
cd D:\FeatherAgent-1.1.8
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

## 4. S3 联网搜索

设置页填：

- Provider：`deepseek`
- Base URL：`https://api.deepseek.com`
- Key 环境变量：`DEEPSEEK_API_KEY`
- 结果条数：5

启动前设置：

```powershell
$env:DEEPSEEK_API_KEY="sk-..."
```

预期：模型可调用 `web.search` / `web.fetch`；返回标题、URL、摘要和 `untrusted:true` 提示。设置页 `search.status` 只显示 key 是否配置，不代表 key 有效或余额充足。

**计费**：DeepSeek Search/Fetch 按成功请求计费，5 元 / 1,000 次；不是免费。失败、空结果、超时不收费。服务端工具搜索不单独收费但消耗模型 token。不想付费就把 `search_provider` 设为 `none`。

## 5. 回归命令

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

## 6. 已知边界

- N2 只是增加 `[init]` 进度，**没有消除**首次杀毒扫描耗时。
- 旧版 `<workspace_root>/<session>/` 空目录不会自动清理；确认无内容后手动删。
- TUI 设置侧栏、WebUI 新控件、DeepSeek 搜索在当前 Linux 环境未做真实 UI/网络端到端，需真机确认。
- v1.1.8 未跑 Wine；Wine 通过也不等于 Windows 通过。
- 仍需复测：高 DPI/中文渲染、真实 Ollama 7B/4B 指标、代理环境、网络盘/同步盘、24h 浸泡。

## 7. 产物校验

```powershell
Get-FileHash .\FeatherAgent-1.1.8-windows-x64.zip -Algorithm SHA256
```

以 Release 资产与 manifest 的字节数/哈希为准；包内 5 个 exe 只依赖 Windows 系统 DLL。
