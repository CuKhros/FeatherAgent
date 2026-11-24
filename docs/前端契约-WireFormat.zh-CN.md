# FeatherAgent 前端契约 · Wire Format v1.0

> 适用版本：FeatherAgent **1.2.1-rc.3.2**（契约 API `1.0`）
> 本文描述**真正在线上传输的字节**，不是设计草案。任何第三方前端（Python / Node / Go / C# / WebSocket 桥）都应按本文实现，不要再靠试错猜测分帧。

## 1. 传输端点

| 平台 | 端点 | 说明 |
|---|---|---|
| Windows | 命名管道 `\\.\pipe\featheragent-<hash>` | 默认 DACL 同用户可访问；`PIPE_REJECT_REMOTE_CLIENTS` |
| Linux / macOS | Unix domain socket `<root>/runtime/ipc/featheragent.sock` | 权限 `0600` |

服务端启动成功后会把**实际端点**写入：

```
<root>/runtime/ipc/endpoint
```

内容示例：

- Windows：`\\.\pipe\featheragent-a49fcfef058c3da5`
- Unix：`/home/user/fa/runtime/ipc/featheragent.sock`

第三方前端应优先读这个文件，不要自己复刻 `blake3` 管道名派生规则。

启动期还有两个状态文件：

```
<root>/runtime/ipc/starting   # serve 已开始初始化，JSON 含 pid/version/started_at_unix_ms
<root>/runtime/ipc/ready      # 已成功绑定端点，可连接；JSON 含 endpoint/pid/version
```

`starting` 存在而 `ready` 不存在 = 内核启动中，不是通道故障。`chat` 会等待最多 5 s 并输出“内核未就绪（启动中，约 Ns）”；第三方前端也应这样提示，不要直接报 `ENOENT`。启动失败/退出时 `starting`、`ready`、`endpoint` 会被清理。

## 2. 分帧：4 字节小端长度 + 消息体

**每一帧**（请求、响应、事件通知都一样）：

```
+----------------+--------------------------------+
| u32 LE length  | body (length bytes)            |
+----------------+--------------------------------+
```

- `length` 是**消息体字节数**，不含这 4 字节帧头。
- 上限 **64 MiB**（67,108,864 B）。超过即断连，错误 `frame too large`。
- 没有 newline 分隔，没有 `Content-Length:` LSP 头，没有 u32-BE。
  报告里用这四种方式探测全部失败，是因为分帧/编码都不匹配，不是服务端“没响应”。

### 2.1 消息体编码

由服务端启动参数决定，**整个连接生命周期内固定**：

```bash
featheragent serve --codec msgpack   # 默认
featheragent serve --codec json      # 调试/第三方参考客户端
```

| codec | body 编码 | 典型首字节 |
|---|---|---|
| `msgpack`（默认） | MessagePack **named map**（字段名做 key，不是数组） | `0x83` / `0x84` 等 `fixmap` |
| `json` | UTF-8 JSON 对象 | `0x7b`（`{`） |

> 关键：**响应使用什么编码，由服务端 `--codec` 决定；客户端必须用同一种编码发请求，并按同一种编码解响应。**
> v1.1.1 的 bug 是 `--codec json` 只影响读、写仍硬编码 MsgPack；1.1.2 已修复，`initialize.result.codec` 会如实返回 `json` 或 `msgpack`。

`initialize` 返回示例（JSON codec）：

```json
{"jsonrpc":"2.0","id":1,"result":{
  "version":"1.2.1-rc.3.2","contract":"1.0","transport":"pipe",
  "codec":"json",
  "paths":{"install_root":"...","data":"...","workspace":"...","runtime":"..."}
}}
```

## 3. JSON-RPC 2.0 信封

请求：

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
```

成功响应：

```json
{"jsonrpc":"2.0","id":1,"result":{...}}
```

错误响应：

```json
{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"..."}}
```

事件通知（无 `id`，服务端主动推送）：

```json
{"jsonrpc":"2.0","method":"event","params":{
  "kind":"model.delta","payload":{"session":"cli","text":"...","first":true},
  "seq":4,"ts":1795331532077826435
}}
```

- `seq` 是连接内事件序号，单调递增。
- `ts` 是 Unix 纳秒时间戳（整数）。

## 4. 握手顺序（必须按这个来）

1. 连接端点。
2. 发 `initialize`，等响应；建议客户端 3 秒超时。
   - 超时通常表示内核未启动、端点文件过期，或 codec 不匹配导致服务端解帧失败。
3. 发 `session.create`（`params.session` 可省略，默认 `cli`）。
4. 发 `event.subscribe`（可选 `kinds` 数组；不传表示全部）。
5. 发 `chat.send`（`params.text` 必填，`params.session` 与第 3 步一致）。
6. 循环读帧，处理 `method == "event"` 的通知，直到 `params.kind == "chat.finished"`。

### 4.1 最小 Python 片段（JSON codec）

```python
import socket, struct, json

def send(sock, obj):
    body = json.dumps(obj, ensure_ascii=False).encode()
    sock.sendall(struct.pack("<I", len(body)) + body)

def recv_exact(sock, n):
    out = b""
    while len(out) < n:
        b = sock.recv(n - len(out))
        if not b:
            raise EOFError
        out += b
    return out

def recv(sock):
    n = struct.unpack("<I", recv_exact(sock, 4))[0]
    return json.loads(recv_exact(sock, n))

s = socket.socket(socket.AF_UNIX)
s.connect("/path/to/runtime/ipc/featheragent.sock")
send(s, {"jsonrpc":"2.0","id":1,"method":"initialize","params":{}})
print(recv(s))
```

Windows 命名管道在 Python 里可用 `open(r'\\.\pipe\featheragent-...', 'r+b', buffering=0)`；读写同样是「4 字节小端长度 + body」，但**不要**用 `readline()`。

## 5. 方法集（契约 1.0）

| 方法 | 作用 | 关键参数 |
|---|---|---|
| `initialize` | 握手、返回版本/路径/实际 codec | 无 |
| `system.info` | 内核状态、告警、门禁 | 无 |
| `session.create` | 创建/复用会话 | `session`（可选）、`workspace`（项目名，可选） |
| `session.list` | 列出会话 | 无 |
| `session.resume` | 恢复已有会话 | `session` |
| `session.fork` | 从某会话分叉 | `session`、`as`、`from_seq` 等 |
| `event.subscribe` | 订阅事件 | `kinds`（可选字符串数组） |
| `chat.send` | 发送用户消息，流式返回事件 | `text` 必填、`session` |
| `chat.stop` / `chat.cancel` | 紧急停止并保留上下文 / 兼容别名 | `session` |
| `chat.inject` | 运行中插入引导（打断后继续） | `session`、`text` |
| `tool.confirm` / `tool.deny` | 确认/拒绝待执行工具 | `token`（来自 `tool.confirm_request`）、`allow` |
| `ctx.inspect` | 查看上下文组装细节 | 见实现 |
| `context.compact` | 手动压缩旧回合为检查点 | `session` |
| `workspace.list` / `workspace.create` | 列出/创建项目工作区 | `name`（仅 create） |
| `search.status` | 联网搜索配置状态（不验证 key 有效性） | 无 |
| `config.get` / `config.set` / `config.replace` | 读取/修改运行配置 | `config.get`：无参数返回完整配置；`path` 支持 `temperature` 或 `/temperature`，返回 `{value, config}`。`config.set`：`{patch}` 或 `{path,value}`。`config.replace`：`{config}` 整份替换 |
| `provider.models` | 读取 provider 的模型列表 | `provider`、`base_url`、`model`（均可选） |
| `storage.stats` / `storage.gc` | 存储统计/回收 | 无 |
| `plugin.list` | 插件清单 | 无 |
| `plugin.enable` / `plugin.disable` | 启停插件 | `name` |
| `plugin.settings` | 读取插件自述的设置 schema 与当前值 | `name` |
| `plugin.install` | 安装插件 | `path`（含 `plugin.json` 的目录绝对路径） |

完整参数以 `src/server/mod.rs` 的 `dispatch` 为准；本文只锁定**传输层**，避免第三方再被分帧卡住。工具调用没有独立 RPC：模型/会话通过 `chat.send` 触发，工具结果以事件形式回传。`model.message` 的 `finish` 可能是 `tool_calls`，随后依次出现 `tool.call` / `tool.result`。业务错误（路径越界/缺参数/文件不存在）为 `tool.error`；只有插件崩溃/超时/IO 故障才为 `tool.failed`；连续 3 次技术失败会为该工具发 `tool.degraded`。provider 必须把结构化 tool_calls 放进 `done` 事件，而不是只拼接文本。

### 5.1 工具确认与拒绝（v1.1.4）

- dangerous 工具需要确认时，内核广播 **notification** `tool.confirm_request`，参数为 `session` / `token` / `tool` / `risk`，并在 v1.2.1-rc.3.1 增加 `arguments`（模型产出的原始 JSON）与 `preview`（人读文本：`tool.shell` 给 `$ <命令>` 加其余参数，其他工具给缩进 JSON，超 4 KB 截断）。前端应展示 `preview`，且必须容忍它缺失（老内核）。
- 前端用 `tool.confirm`（`{"token":"...","allow":true}`）或 `tool.deny`（`{"token":"..."}`）响应。
- 拒绝时除 `permission.denied` 外，还会写入一条结构化 `tool.result`：`{"denied":true,"result":{"error":"permission_denied","reason":"user_denied|确认超时","tool":"..."}}`，模型下一轮可据此解释。
- `chat` 命令支持 `--allow <tool,...>` / `--yes` 自动确认；未放行时默认立即 deny（`--confirm-timeout` 可保留人工窗口）。
- 空响应自动重试一次时写入 `model.retry` 事件（`attempt` / `reason` / `temperature`）；仍失败则 `chat.finished status=failed`。

### 5.2 设置与模型发现（v1.1.7）

前端设置页统一走以下链路，不要直接改 `config.json`：

```json
// 1) 读取
{"method":"config.get","params":{}}
// → {"provider_plugin":"provider-ollama","base_url":"...","model":"qwen3.5:4b",
//    "temperature":0.7,"context_window":8192,"max_turns":6,"auto_allow_safe":true,
//    "workspace_root":"",...}

// 2) 发现模型（Ollama 读 /api/tags；OpenAI 兼容读 /v1/models）
{"method":"provider.models","params":{"provider":"provider-ollama","base_url":"http://127.0.0.1:11434","model":"qwen3.5:4b"}}
// → {"provider":"provider-ollama","models":["qwen3.5:4b","..."],"probe":{...}}
// models 已去重；失败时仍返回 200：{"provider":"...","models":[],"error":"..."}，前端应允许手填模型

// 3a) 保存单个字段（原子读-改-写，推荐脚本/多前端）
{"method":"config.set","params":{"path":"temperature","value":0.6}}
// → {"config":{...完整新配置...}}

// 3b) 保存多个字段（只提交需要改的字段；WebUI/TUI 使用）
{"method":"config.set","params":{"patch":{
  "provider_plugin":"provider-ollama","model":"qwen3.5:4b","temperature":0.6,
  "context_window":8192,"max_turns":6,"auto_allow_safe":true
}}}
// → {"config":{...完整新配置...}}

// 3c) 整份替换（迁移/导入）
{"method":"config.replace","params":{"config":{...完整 Config...}}}
```

- `config.set` / `config.replace` 都在内核配置锁内完成“读 → 改 → 校验 → 临时文件 + 原子替换 → 更新内存 → 发 `config.changed`”，因此两个前端并发保存不会后写覆盖前写；`config.set` 的 `path` 支持 `temperature` 或 `/temperature`，未知字段返回 `-32602`。
- `config.replace` 才表示整份替换；不要把 `config.set` 的 patch 当作 replace。
- 温度范围 0.0–2.0；`context_window` 512–131072；`max_turns` 1–32；`provider_plugin` 必须是已登记插件，否则服务端返回 `-32602`。
- 空响应重试仍固定 `temperature=0.7`，与用户设置无关。
- `workspace_root` 为空表示 `<安装根>/workspace`；相对路径按安装根解析，绝对路径可指向安装目录外。它是**项目容器**，不是某个会话的沙箱：容器下一层 `<workspace_root>/<project>/` 才是工作区。它是**启动期配置**：`config.set` 会持久化并立即返回新配置，但运行中的内核不热迁移工作区，需重启 `serve`。
- 每个项目工作区各自拥有 `<workspace_root>/<project>/.featheagent/{worklog.md,memory.md,README.md}`（不覆盖已有内容）。内核自动追加 worklog（真实 UTC 时间、目标、工具、状态），模型不手写时间；容器根不再创建 `.featheagent`。
- 会话创建时可传 `workspace` 绑定项目；旧日志无 workspace 时回落 `default`。工具路径 `workspace/xxx` 与 `xxx` 都落在当前项目根。
- `ctx.inspect` 是诊断接口，不保证拿到会话锁：锁繁忙时返回 `state:"busy"`、`snapshot:null`，前端不要据此判定回合失败。

### 5.3 插件管理（v1.1.7）

插件管理完全走 RPC，不直接改 `data/plugins/registry.json`：

```json
// 列表：registry 是登记表，live 是当前运行状态
{"method":"plugin.list","params":{}}
// → {"registry":[{"name":"tool","program":"...","args":[],"category":"tool",
//                 "enabled":true,"blake3":"..."}],
//    "live":[{"name":"tool","state":"ready|stopped","manifest":{...}}]}

// 安装：path 是含 plugin.json 的目录绝对路径
{"method":"plugin.install","params":{"path":"D:\\plugins\\my-tool"}}
// → {"installed":{"name":"my-tool","program":"...","enabled":true,...}}

// 启停：同步 registry 与运行进程
{"method":"plugin.enable","params":{"name":"my-tool"}}
{"method":"plugin.disable","params":{"name":"my-tool"}}

// 设置：插件自述 schema，内核原样转述
{"method":"plugin.settings","params":{"name":"review-gate"}}
// → {"name":"review-gate","root":"review_gate","title":"...","description":"...",
//    "fields":[{"key":"max_rounds","label":"最大修订轮次","type":"int",
//               "default":2,"min":1,"max":5,"step":1}, ...],
//    "values":{"enabled":false,"max_rounds":3,...},
//    "defaults":{...}}
```

- `plugin.install` 成功且插件启用时，会立即把 spec 注册进运行 host；`plugin.enable` 重新登记，按需拉起；`plugin.disable` 会终止已运行进程。
- `plugin.list` 的 `registry` 可用于生成动态侧栏；`live.state` 为 `ready` 表示已运行，`stopped` 表示按需启动。
- 插件 manifest 的 `api` 必须落在契约 `>=1.0 <2.0`；不满足时安装失败。
- `manifest.settings`（可选，v1.2.1-rc.3）是插件设置界面的唯一来源：
  - `root`——这批字段落在哪个配置路径（缺省 `plugin_settings.<插件名>`）；
  - `title` / `description`——表单上方显示什么；
  - `fields[]`——`key`、可选绝对 `path`（用于指向既有顶层键）、`label`、`type`（只允许 `bool|int|number|string|enum`）、`default`、`description`、`placeholder`、`min`、`max`、`step`、`options`、`options_source`（`""` 或 `"models"`）、`allow_custom`、`secret`。
  - `options_source:"models"` 不代表内核解析列表：内核只转述声明，前端自己调既有 `provider.models` RPC 并渲染选择器；`allow_custom:true` 表示列表外的值仍必须能保存（服务没报的模型名要能手动填）。
- 未声明设置项的插件仍会出现在 `plugin.list`；`plugin.settings` 对它返回业务错误（“插件 X 未声明设置项”），前端应回退到清单摘要 + 启用开关。
- 写入走 `config.set`，路径为展开后的 `<root>.<key>`；内核校验类型/区间/枚举，并拒绝 `plugin_settings.<name>` 下未声明的键；首次写声明路径会自动补父级。前端不得硬编码任何插件的字段名，一律按 `plugin.settings` 渲染。
- `registry[]` 条目自带 `settings`（同一份 schema），因此侧边栏可以在插件进程启动前就建好。

### 5.4 工具错误事件

| kind | 语义 | 是否计入熔断 | 前端建议 |
|---|---|---|---|
| `tool.result` | 工具成功返回，或权限拒绝的结构化结果（`denied=true`） | 否 | 展示结果；拒绝标红 |
| `tool.error` | 工具业务错误（路径越界/缺参数/文件不存在） | **否** | 展示“业务错误”，模型会收到 tool 消息 |
| `tool.failed` | 插件崩溃/超时/IO 故障 | 是（按工具名） | 展示“插件失败”；连续 3 次后显示熔断 |
| `tool.degraded` | 某个工具连续 3 次技术失败被熔断 | — | 该工具从后续 tools schema 移除，system 注入“不要输出 tool_call 语法” |

## 6. 错误码

| code | 含义 |
|---|---|
| `-32601` | 方法不存在 |
| `-32602` | 参数非法 |
| `-32603` | 内核内部错误 |
| `40301` | 观察者只读，禁止写操作 |
| `40401` | 会话不存在 |
| `42901` | 预算/配额超限 |
| `50201` | 插件调用失败 |
| `40901` | 资源被占用（如单客户端能力被占） |

## 7. 多客户端与断连语义（1.1.2 起；1.1.3 补取消，1.1.4 补确认，1.1.5 补设置，1.1.7 补插件/工作区，1.1.8 补停止/引导/压缩/联网）

- Windows 命名管道使用 **overlapped I/O + 每连接独立句柄**，读写互不阻塞。
- `accept` 遇到客户端连上后立即断开（`ERROR_NO_DATA`）会复位实例并返回错误，由 `Server::run` 继续 `accept`；不会永久占死实例。
- 客户端读线程看到 `ERROR_BROKEN_PIPE` / `ERROR_PIPE_NOT_CONNECTED` 按 EOF 处理。
- WebUI / TUI / 第三方客户端可以并发连接；事件总线按连接独立推送。
- 单客户端写操作（如某些治理动作）仍可能返回 `40901`，这是契约语义，不是管道死锁。
- 1.1.3 起，连接断开时若会话正处于 `Assembling` / `Generating` / `ToolExecuting`，内核会取消当前请求、发 `tool.cancelled`（`reason=client_disconnect`）并释放会话 holder；`Cancelled` / `Failed` 状态允许下一次 `chat.send` 重新进入。

## 8. 兼容性边界

- `--codec json` 是**调试/参考实现**能力；WebUI 与 TUI 内置客户端固定使用 MsgPack，默认 `serve` 即可。
- 第三方若选 JSON，必须让服务端也用 `--codec json`，并在 `initialize.result.codec` 校验一致；不一致时不要继续发请求。
- 帧头/上限/方法集属于契约 `1.0`；新增方法只增不改，破坏性变更才升 `CONTRACT_API`。


### 5.4 项目工作区、停止/引导、压缩与联网（v1.1.8）

```json
// 项目容器：root 是容器，workspaces 是容器下一层项目
{"method":"workspace.list","params":{}}
// → {"root":".../workspace","workspaces":[{"name":"default","path":".../default",
//    "has_agent":true,"has_hidden":false}]}

{"method":"workspace.create","params":{"name":"proj-a"}}
// → {"ok":true,"name":"proj-a","path":".../workspace/proj-a","workspaces":[...]}

// 会话绑定项目
{"method":"session.create","params":{"session":"s1","workspace":"proj-a"}}
// → {"session":"s1","workspace":"proj-a"}

// 紧急停止：保留已生成文本；status=stopped（chat.cancel 兼容返回 cancelled）
{"method":"chat.stop","params":{"session":"s1"}}
// → {"ok":true,"preserve_context":true}

// 回合内引导：仅在运行中可用
{"method":"chat.inject","params":{"session":"s1","text":"先别改代码，先列计划"}}
// → {"ok":true,"during_turn":true}

// 手动压缩：至少两个 ChatFinished；保留最后两个完整回合
{"method":"context.compact","params":{"session":"s1"}}
// → {"ok":true,"summary":"...","covers_until_seq":42,"compacted_events":30,
//    "before_chars":9000,"after_chars":600}

// 联网搜索配置状态（configured 只表示环境变量非空）
{"method":"search.status","params":{}}
// → {"provider":"deepseek","base_url":"https://api.deepseek.com",
//    "key_env":"DEEPSEEK_API_KEY","configured":false,"count":5,
//    "billing":"Search/Fetch 按成功请求计费，5 元/1000 次"}
```

- `chat.finished.status` 新增 `stopped`；旧客户端应把 `stopped` 与 `cancelled` 都视为“被用户停止”。部分输出以 `model.message(finish=interrupted)` 保留。
- 1.2.1-rc.3.2：没有新增消息或字段。`review.hardcheck`（插件内部，不是 wire）对 `min_points` 不足改为返回 `soft` 数组；引擎打 `review.warn`（`text` 以“min_points 机器计数不足，交 judge 复核”开头），并给 judge 输入追加机器计数提示，由 judge 按答案裁决，不再硬拒绝。`review.feedback.text` 改用 `〔1〕`/`〔2〕` 标注未通过项，不再出现裸 `c1:`；事件 payload（`round`/`failed`/`text`/`next_temperature`）不变。
- 1.2.1-rc.3.1：`review.guide`（25）、`review.verdict`（26）、`review.finished`（27）、`review.feedback`（28）之后新增 `review.warn`（29，payload `{text, model, attempts, retry?}`）。warn 只表示“用较弱判据评估”或“这次判据被拒并重试”，不表示门禁失败；只有描述项判据会走 judge-only 并告警，完全产不出判据仍以 `degraded` 收尾。`review.guide` payload 的 `attempts` 标明是第几次产出的判据；`review.guide` 插件调用新增可选 `feedback` 参数，用于回注上一轮校验错误。
- 1.2.1-rc.2.1：review-gate 启用且回合正常结束时，`chat.finished.status` 为 `ok`（approved）、`unapproved`（达到轮次上限仍未通过）或 `degraded`（guide/judge 连续两次失败）。payload 的 `review.status` 同步为 `approved|unapproved|degraded`；`review.verdict.items[]` 除 `evidence` 外新增 `criterion` 与 `check`。客户端不得再假设 `ok` 一定代表“已评审通过”。

- `chat.inject` 先写 `user.message`（`source:"steer"`、`during_turn:true`），再打断当前生成；若会话空闲返回业务错误，请改用 `chat.send`。
- 压缩写 `context.compacted` 事件（码 24），payload 含 `summary` / `covers_until_seq` / `compacted_events` / `before_chars` / `after_chars`；模型视图只保留 cutoff 之后的事件，旧原文仍在 append-only 日志。
- `web.search` / `web.fetch` / `web.news` / `web.suggest` / `web.status` 在 `search_provider=none` 时不出现；`local` 不需要 key，`deepseek` 仍需 key。网页返回一律 `untrusted:true`。本地搜索为零 Key，DeepSeek 计费详见 `docs/v1.2.0-rc.1-变更说明.zh-CN.md` §2。
- `web.search` / `web.news` 的 `count` 在 schema 中声明为 `integer`；小模型发送数字字符串（如 `"5"`）时，内核在转发插件前归一化为整数并保留原 `tool.call` 参数，客户端不应依赖字符串类型。
