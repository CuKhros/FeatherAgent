# FeatherAgent 1.1.1 · Windows x64 测试包

这是给 Windows 10/11 x64 的本地测试包：5 个原生 `.exe` + 出厂插件 + Web 资源，**无 Node / Python / JVM / VC++ 运行库依赖**。

> 构建方式：Linux 上 `x86_64-pc-windows-gnu` 交叉编译 + 静态 CRT。已在 Wine 下跑通 `version / doctor / serve / GUI 桥 / 命名管道 RPC / 配置重写 / 50 次日志截断恢复`；**尚未在真实 Windows 机器上运行**，性能数字以你本机 `bench` 输出为准。

## 1. 解压

解压到一个**有写权限**的普通目录，例如：

```
D:\FeatherAgent\
```

不要放在 `C:\Program Files\`（内核默认把 `data/`、`workspace/`、`runtime/` 建在安装根下，只读目录会失败）。

如果 SmartScreen 提示“已保护你的电脑”，点“更多信息 → 仍要运行”（本包未做代码签名）。

## 2. 启动内核（PowerShell）

```powershell
cd D:\FeatherAgent
.\featheragent.exe serve --root .
```

看到 `featheragent 1.1.1  socket=...  data=...  provider=provider-mock` 即已监听命名管道。首次运行会在当前目录生成 `data/`、`workspace/`、`runtime/`。

## 3. TUI（另开一个 PowerShell）

```powershell
cd D:\FeatherAgent
.\featheragent-tui.exe --root .
```

建议用 Windows Terminal 打开；旧版 conhost 的 UTF-8 / 按键支持较差。

## 4. GUI

```powershell
cd D:\FeatherAgent
.\featheragent-gui.exe --root . --app
```

- `--app` 会自动探测 Chrome / Edge，用独立窗口打开（独立 profile 在 `runtime/gui-profile`）。
- 不加 `--app` 时只起 HTTP 桥并打印带 token 的 `http://127.0.0.1:<port>/#token=...`，复制到浏览器打开即可。
- 不依赖内核已启动；GUI 自己会连命名管道。若内核未启动，先按第 2 步启动。

## 5. 自检

```powershell
.\featheragent.exe doctor --root .
.\featheragent.exe doctor --root . --stress-log 100
.\featheragent.exe bench --json
```

`doctor` 应输出 `"ok": true`；`--stress-log` 跑随机截断恢复（100 次足够冒烟，1000 次较慢）；`bench` 输出本机体积 / 启动 / RSS / 写入 / 组装 / RPC 指标。

## 6. 接真实 Ollama（可选）

先确保本机 `ollama serve` 在 `127.0.0.1:11434`：

```powershell
ollama serve
.\featheragent.exe serve --root . --provider provider-ollama --model qwen2.5:7b
```

## 已知边界

- **真机未测**：本次仅在 Linux 交叉编译 + Wine 冒烟，Windows 实机行为（尤其 TUI 键盘、GUI `--app` 拉起浏览器、SmartScreen）需要你验证。
- **性能未测**：Wine 的启动 / 写入数字受兼容层影响很大，不作为 Windows 性能结论；以你本机 `bench` 为准。
- `tool.shell` 在 Windows 用 `cmd.exe /C`，超时用 `taskkill /F /T /PID` 杀进程树。
- 插件清单里的 `../../feather-plugin` 在 Windows 会自动回退到 `feather-plugin.exe`；不要重命名 exe。
- 卸载 = 删除整个目录（无注册表、无服务、无后台常驻）。
