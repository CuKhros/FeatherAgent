# FeatherAgent 1.1.6 · Windows x64 修复版测试包

这是给 Windows 10/11 x64 的本地测试包：5 个原生 `.exe` + 出厂插件 + WebUI 资源，**无 Node / Python / JVM / VC++ 运行库依赖**。

> v1.1.6 基于你方《Windows 真机复测报告 v1.1.5》：NEW-E～NEW-G 已真机确认修复。本版处理 NEW-H～NEW-J，并修复 WebUI 模型下拉只显示一项的问题。**仍需真机复测**；Wine 只能验证基本启动，不能复现真实 Windows 的管道并发、ConPTY 与代理环境。

## 1. 解压

解压到一个**有写权限**的普通目录，例如：

```
D:\FeatherAgent-1.1.6\
```

不要放在 `C:\Program Files\`（内核默认把 `data/`、`workspace/`、`runtime/` 建在安装根下，只读目录会失败）。

如果 SmartScreen 提示“已保护你的电脑”，点“更多信息 → 仍要运行”（本包未做代码签名）。

## 2. 启动内核（PowerShell）

```powershell
cd D:\FeatherAgent-1.1.6
.\featheragent.exe help
.\featheragent.exe serve --root .
```

启动后会**立即**看到：

```
featheragent 1.1.6 starting  socket=...  data=...  provider=provider-mock  codec=msgpack
serve: 初始化内核（首次启动需登记内置插件，Windows 首次可能 30–60s）
```

随后出现：

```
serve: 内核初始化完成（... ms）
serve: 开始监听 ...（等待 runtime/ipc/ready）
serve ready: 1.1.6
  endpoint=\\.\pipe\featheragent-...
```

- 首次启动可能 30–60 s（内置插件登记 + Windows 实时防护扫描），这是启动等待，不是挂死。
- 进入可用状态的标志：输出 `serve ready`，且 `runtime\ipc\ready` 出现；此前 `chat` 会明确提示“内核未就绪（启动中，约 Ns）”，不会再报误导性的“找不到 .sock 文件”。
- 服务端会把实际命名管道端点写入 `runtime\ipc\endpoint`；第三方客户端直接读它，不要自己算哈希。
- 如果 `runtime\ipc\starting` 存在超过 180 s 且没有 `ready`，`chat` 会提示进程可能已退出，请检查 `serve` 日志。

## 3. TUI（另开一个 PowerShell）

```powershell
cd D:\FeatherAgent-1.1.6
.\featheragent-tui.exe --root .
```

TUI 初始化最多等 3 秒；如果内核没起或管道不通，会明确打印“初始化失败（3s 超时/内核未就绪）”并退出，不再永久黑屏。建议用 Windows Terminal。

输入 `/settings`（或 `/config`）打开设置页：`Tab` / `↑↓` 切换字段，`←→` 选择 Provider/模型或调整数值，文本字段直接输入；`Ctrl+S` 或 `Enter` 保存并应用，`Esc` 返回。

## 4. WebUI（浏览器形态）

```powershell
cd D:\FeatherAgent-1.1.6
.\featheragent-webui.exe --root . --app
```

- `--app` 会自动探测 Chrome / Edge，用独立窗口打开（独立 profile 在 `runtime/webui-profile`）。
- 不加 `--app` 时只起 HTTP 桥并打印带 token 的 `http://127.0.0.1:<port>/#token=...`。
- WebUI 连接内核最多等 3 秒；内核未启动会报错退出，不再静默挂起。token 由系统 CSPRNG 生成。
- 右上角 `⚙ 设置` 打开设置页；可配置 Provider、Base URL、模型、温度、上下文窗口、单轮回合上限与“自动放行安全工具”。
- **模型控件现在是一个真正的下拉列表**，列出 `provider.models` 返回的全部模型（Ollama `/api/tags`，OpenAI 兼容 `/v1/models`）；左侧输入框仍可手填。不会因为当前值已填好而只显示一项。
- 点“保存并应用”后，WebUI 调用 `config.set`（`{patch}`），内核写入 `data/config.json` 并立即用于下一轮推理；不需要重启内核。
- 若两个前端同时保存，内核用配置锁串行化读-改-写；只改一个字段时也可用 `config.set {path,value}`，避免整份覆盖。
- 旧命令 `featheragent gui` 仍保留兼容别名；**真正的原生 GUI 计划作为独立插件**，不与当前 WebUI 混名。

## 5. 无 WebUI/TUI 的脚本通道

```powershell
.\featheragent.exe chat --root . --text "hello"
```

`chat` 会自己完成 `initialize` → `session.create` → `event.subscribe` → `chat.send`，逐行打印事件直到 `chat.finished`。`chat.finished` 为 `failed` 时命令返回非零退出码，并打印错误原因。

**危险工具授权（NEW-A/B）**：

```powershell
# 只放行写盘；shell 仍需人工/前端确认
.\featheragent.exe chat --root . --session s3 --text "写文件" --allow tool.fs.write

# 受信 CI：放行所有 dangerous 工具（谨慎）
.\featheragent.exe chat --root . --session s3 --text "跑 shell" --yes

# 不放行时默认立即拒绝；如需给人工 5 秒确认窗口
.\featheragent.exe chat --root . --session s3 --text "写文件" --confirm-timeout 5
```

- 未加 `--allow` / `--yes` 时，`chat` 对 `tool.confirm_request` **立即发 deny**，不再等 120 秒。
- 拒绝会同时产生 `permission.denied` 和结构化 `tool.result`（`{"error":"permission_denied","reason":"..."}`），模型能据此解释“缺少授权”，不会只看到工具消失。
- `--allow` 支持逗号分隔，如 `--allow tool.fs.read,tool.fs.write`；也接受短名 `fs.write`。
- `--yes` 只建议用于受信脚本/CI；TUI/WebUI 仍走正常确认流程。

若第三方要用 JSON 调试：

```powershell
.\featheragent.exe serve --root . --codec json
```

然后用 `tools\feather_client.py`（需 Python 3）或按 `docs\前端契约-WireFormat.md` 自己实现。**默认 `serve` 是 MsgPack**，WebUI/TUI 也只支持 MsgPack；JSON 与 MsgPack 不能混用。

## 6. 自检与平台门禁

```powershell
.\featheragent.exe doctor --root .
.\featheragent.exe doctor --root . --stress-log 100
.\featheragent.exe bench --root . --json
```

| 门禁 | Windows 限值 | Linux 限值 |
|---|---|---|
| 内核体积 | ≤ 8 MB | ≤ 8 MB |
| 进程内启动 P50 | **< 100 ms** | < 20 ms |
| 空闲 RSS | ≤ 15 MB | ≤ 15 MB |
| 事件写入 P95 | **< 10 ms** | < 100 µs |
| 上下文组装 P95 | < 30 ms | < 30 ms |
| RPC 往返 P95 | < 1 ms | < 1 ms |

`bench --json` 会返回 `platform`、`boot_limit_ms`、`write_limit_us`，不会把 Windows 固定标红。

Windows NTFS + 实时防护下 `FlushFileBuffers` 很贵，报告里的 4.1 ms/次是**环境特性而非实现退化**；Linux 仍按 100 µs 门禁。

## 7. 接真实 Ollama（S3）

先确保本机 `ollama serve` 在 `127.0.0.1:11434`，且模型已拉取：

```powershell
ollama serve
ollama list
.\featheragent.exe serve --root . --provider provider-ollama --model qwen3.5:4b
.\featheragent.exe chat --root . --session s3a --text "请用工具读取 workspace/s3-probe.txt，把文件内容原样告诉我。"
```

写盘 / shell 类任务按 §5 加 `--allow tool.fs.write,tool.shell`。空响应会自动重试一次（`model.retry` 事件）；仍为空则 `chat.finished status=failed`，不会假成功。

包内 `tools\s3_batch.py` 已适配新版 `chat`，默认通过 `--allow tool.fs.write,tool.shell` 放行写盘与 shell；如需改白名单，设置环境变量 `FEATHER_ALLOW=tool.fs.read` 后再运行。

脚本采用**三档口径**：

- `OK`：有 `tool.result`、未被 denied、result 无 error，且无 `tool.error`/`tool.failed`。
- `预期失败`：负样本（如读不存在的文件）产生业务错误，且模型最终答复正确。
- `ERR`：其余情况，包括真失败与“预期错误但没有正确报告”。

因此 t6 不会再被错误计入“真失败”；统计会同时给出“工具执行成功 4/5、预期业务错误 1/1、真失败 0/5”。

## 8. 设置 API（脚本/第三方）

```json
// 单字段原子更新
{"method":"config.set","params":{"path":"temperature","value":0.42}}

// 只提交需要改的字段（推荐前端使用）
{"method":"config.set","params":{"patch":{"model":"qwen3.5:4b","temperature":0.4}}}

// 整份替换（迁移/导入）
{"method":"config.replace","params":{"config":{...完整 Config...}}}
```

`config.get` 的 `path` 支持 `temperature` 或 `/temperature`。所有写路径都在内核配置锁内完成，写盘为临时文件 + 原子替换。

## 已知边界

- **v1.1.6 真机未测**：NEW-H/I/J、WebUI 多模型下拉、设置页在 Windows 真机需复测；NEW-4 断连取消在 v1.1.3/1.1.4 报告中标为“未复测”，本版未改该路径。
- **工具错误语义**：`tool.error` 是业务错误（路径越界/缺参数/文件不存在），不熔断；`tool.failed` 才代表插件崩溃/超时/IO 故障。连续 3 次技术失败只熔断对应工具。
- **Wine 冒烟**：v1.1.6 的 `version` / `help` / `doctor` / `bench` 在 Wine 6.0.3 通过；在同一包目录下 `serve` 5 s 内输出 banner/`ready`，单客户端 `chat` 返回 `echo: hello` / `status=ok`。boot 142–160 ms、RSS 17,152–17,280 KiB 两项受 Wine 开销影响未过 Windows 门禁，write P95 4.5–4.7 ms、assemble P95 0.146–0.160 ms、RPC P95 0.212–0.278 ms 通过。并发命名管道、ConPTY、真实代理/杀进程不作 Windows 结论；Wine 通过不等于真机通过。
- **工具调用**：同时兼容 Ollama 原生 `message.tool_calls`（`arguments` 为对象）与 OpenAI 兼容 `/v1` 的 `delta.tool_calls`（`arguments` 为 JSON 字符串）；`workspace/xxx`、`/workspace/xxx`、`\workspace\xxx` 均按安装根解析，普通相对路径按会话工作区解析。
- **代理**：插件子进程会保留已有 `NO_PROXY` 并补齐 `127.0.0.1,localhost,::1`；HTTP 客户端本身是直连 TCP，不读取 `HTTP_PROXY`。
- `tool.shell` 在 Windows 用 `cmd.exe /C`，超时用 `taskkill /F /T /PID` 杀进程树。
- 插件清单里的 `../../feather-plugin` 在 Windows 会自动回退到 `feather-plugin.exe`；不要重命名 exe。
- 卸载 = 删除整个目录（无注册表、无服务、无后台常驻）。
