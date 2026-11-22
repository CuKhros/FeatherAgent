# FeatherAgent 1.1.3 · Windows x64 修复版测试包

这是给 Windows 10/11 x64 的本地测试包：5 个原生 `.exe` + 出厂插件 + Web 资源，**无 Node / Python / JVM / VC++ 运行库依赖**。

> v1.1.3 基于你方《Windows 真机复测报告 v1.1.2》的 NEW-1 ～ NEW-4 修复：原生/OpenAI `tool_calls` 结构化解析、空响应报错、插件子进程 `NO_PROXY` 回环、bench 门禁分平台、断连取消与会话释放。**v1.1.3 仍需你在真机复测**；Wine 只能验证基本启动/管道，不能复现真实 Windows 的管道并发、ConPTY 与代理环境。

## 1. 解压

解压到一个**有写权限**的普通目录，例如：

```
D:\FeatherAgent-1.1.3\
```

不要放在 `C:\Program Files\`（内核默认把 `data/`、`workspace/`、`runtime/` 建在安装根下，只读目录会失败）。

如果 SmartScreen 提示“已保护你的电脑”，点“更多信息 → 仍要运行”（本包未做代码签名）。

## 2. 启动内核（PowerShell）

```powershell
cd D:\FeatherAgent-1.1.3
.\featheragent.exe help
.\featheragent.exe serve --root .
```

看到 `featheragent 1.1.3  socket=...  data=...  provider=provider-mock  codec=msgpack` 即已监听命名管道。首次运行会在当前目录生成 `data/`、`workspace/`、`runtime/`。

服务端会把**实际管道端点**写入 `runtime\ipc\endpoint`，第三方客户端直接读它，不要自己算哈希。

## 3. TUI（另开一个 PowerShell）

```powershell
cd D:\FeatherAgent-1.1.3
.\featheragent-tui.exe --root .
```

v1.1.3 的 TUI 在初始化阶段最多等 3 秒；如果内核没起或管道不通，会明确打印“初始化失败（3s 超时/内核未就绪）”并退出，不再永久黑屏。建议用 Windows Terminal。

## 4. GUI

```powershell
cd D:\FeatherAgent-1.1.3
.\featheragent-gui.exe --root . --app
```

- `--app` 会自动探测 Chrome / Edge，用独立窗口打开（独立 profile 在 `runtime/gui-profile`）。
- 不加 `--app` 时只起 HTTP 桥并打印带 token 的 `http://127.0.0.1:<port>/#token=...`。
- v1.1.3 的 GUI 连接内核最多等 3 秒；内核未启动会报错退出，不再静默挂起。token 由系统 CSPRNG 生成，不会再出现 48 个 0。

## 5. 无 GUI/TUI 的脚本通道

```powershell
.\featheragent.exe chat --root . --text "hello"
```

`chat` 会自己完成 `initialize` → `session.create` → `event.subscribe` → `chat.send`，逐行打印事件直到 `chat.finished`。适合 SSH、CI、以及接真实 Ollama 的 S3 测试。v1.1.3 起，`chat.finished` 为 `failed` 时命令返回非零退出码，并打印错误原因，不会再把空响应当成功。

若第三方要用 JSON 调试：

```powershell
.\featheragent.exe serve --root . --codec json
```

然后用 `tools\feather_client.py`（需 Python 3）或按 `docs\前端契约-WireFormat.md` 自己实现。**默认 `serve` 是 MsgPack**，GUI/TUI 也只支持 MsgPack；JSON 与 MsgPack 不能混用。

## 6. 自检

```powershell
.\featheragent.exe doctor --root .
.\featheragent.exe doctor --root . --stress-log 100
.\featheragent.exe bench --root . --json
```

- `doctor` 应输出 `"ok": true`。
- `bench` 会打印 `bench[1/6]` ～ `bench[6/6]` 进度；v1.1.3 修复了 stop 唤醒逻辑，理论上不再 180 秒挂死。若仍挂，请把最后一行进度和 `serve.log` 一起发回。
- Windows 门禁分档：启动 P50 **<100 ms**、事件写 P95 **<10 ms**（Linux 仍是 20 ms / 100 µs）；JSON 里会带 `platform` 与 `boot_limit_ms`/`write_limit_us`。
- Windows NTFS + 实时防护下 `FlushFileBuffers` 很贵，报告里的 4.5 ms/次是预期现象；v1.1.3 未改变持久化策略，只补了可观测性。

## 7. 接真实 Ollama（S3）

先确保本机 `ollama serve` 在 `127.0.0.1:11434`，且模型已拉取：

```powershell
ollama serve
ollama list
.\featheragent.exe serve --root . --provider provider-ollama --model qwen3.5:4b
.\featheragent.exe chat --root . --text "用一句话解释 Rust 所有权"
```

## 已知边界

- **v1.1.3 真机未测**：本次修复依据 v1.1.2 复测报告，需要你在同一台 Windows 上复测 NEW-1 ～ NEW-4，并顺带回归 BUG-1 ～ BUG-9。
- **Wine 局限**：Wine 6 的命名管道并发语义与真实 Windows 不同，Wine 冒烟通过不等于真机通过。
- **工具调用**：v1.1.3 同时兼容 Ollama 原生 `message.tool_calls`（`arguments` 为对象）与 OpenAI 兼容 `/v1` 的 `delta.tool_calls`（`arguments` 为 JSON 字符串）；模型给的 `workspace/xxx` 会按安装根解析，普通相对路径按会话工作区解析。
- **代理**：插件子进程会保留已有 `NO_PROXY` 并补齐 `127.0.0.1,localhost,::1`；HTTP 客户端本身是直连 TCP，不读取 `HTTP_PROXY`。若仍有空响应，`chat.finished` 会是 `failed` 并带原因。
- `tool.shell` 在 Windows 用 `cmd.exe /C`，超时用 `taskkill /F /T /PID` 杀进程树。
- 插件清单里的 `../../feather-plugin` 在 Windows 会自动回退到 `feather-plugin.exe`；不要重命名 exe。
- 卸载 = 删除整个目录（无注册表、无服务、无后台常驻）。
