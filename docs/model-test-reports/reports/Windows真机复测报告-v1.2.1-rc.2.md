# FeatherAgent v1.2.1-rc.2 · Windows 真机复测报告

> 测试机：Windows x64（同前）· Ollama 0.32.9
> 复测时间：2026-11-24
> 被测包：`FeatherAgent-1.2.1-rc.2-windows-x64`
> 对照：`Windows真机复测报告-v1.2.1-rc.1.md`（P0/P1/P2）+ §11 review-gate 设计评审

---

## 1. 结论

**P0 / P1 / P2 三项全部修复并验证，其中 P1 的错误文案与 §11.4 建议逐字一致。**
**R1 保持关闭（7/7 门禁全绿）；S3 恢复满分。**
**同时第一次拿到「review-gate 在真实模型上」的实测数据——结论是：机制完整，但当前配置下必然"未经批准"，纯亏成本。**

| 项 | rc.1 | rc.2 | 证据 |
|---|---|---|---|
| **P0 空 body** | 🔴 全线 400 | ✅ **已修** | `/api/chat` `Content-Length=6964`、`/api/embed` `=45`，body 完整 |
| **P1 温度区间** | ❌ 照单全收 | ✅ **已修** | 越界返回 `超出允许区间 [lo, hi]`，且**关闭状态下也生效** |
| **P2 配置优先级** | 🟡 静默覆盖 | ✅ **已修** | 3 条 `warn` + `effective model=... (model_source=cli...)` |
| R1 冷启动 | ✅ | ✅ **保持** | 热 45.3 ms / 冷 173.25 ms，**7/7 全绿** |
| S3 | ⛔ 无法测 | ✅ **满分** | 触发 5/5 · 执行 4/5 · 真失败 0/5 · 写盘 OK |
| review-gate | mock 通过 | ⚠️ **真实模型必然不通过** | 见 §5 |

---

## 2. P0 验证：请求体修复（抓包实证）

用本地 echo 服务（`--base-url http://127.0.0.1:11499`）抓内核实际发出的请求：

```
PATH /api/embed  Content-Length=45    BODY_LEN=45
  {"input":"只回答 OK","model":"qwen3.5:4b"}

PATH /api/embed  Content-Length=45    BODY_LEN=45
  {"input":"只回答 OK","model":"qwen3.5:4b"}

PATH /api/chat   Content-Length=6964  BODY_LEN=6964
  {"messages":[{"content":"你是 FeatherAgent 的助手，服务于本地 qwen3.5:4b 模型…"}]}
```

**对比 rc.1（同样的抓包手段）：**

| | rc.1 | rc.2 |
|---|---|---|
| `/api/embed` | `Content-Length: 0` `BODY[0]: b''` | **45 字节，含 input+model** |
| `/api/chat` | `Content-Length: 0` `BODY[0]: b''` | **6964 字节，messages 完整** |

**真实 Ollama 上也通了：**

```
model.delta   {"text":"OK"}
model.message {"finish":"stop","text":"OK"}
```

**根因说明（来自 changelog，与实测吻合）**：空 API key 在 `Content-Length` 前插入了空行，
HTTP 头提前终止，服务端把 body 当成不存在。改为逐行追加头之后，body 与 `Content-Length` 始终存在。

> 附带确认：`/api/embed` 现在带 `model` 字段，即 changelog 说的"embeddings 使用当前模型"也已生效。

---

## 3. P1 验证：温度区间（错误文案与建议一致）

**在 `review_gate.enabled=false` 的状态下**测试（changelog 承诺"regardless of enabled"）：

```
review_gate":{"enabled":false          ← 确认是关闭态

guide 0.9 -> {"code":500,"message":"rpc -32602: review_gate.guide.temperature=0.9 超出允许区间 [0.2, 0.5]"}
exec  1.9 -> {"code":500,"message":"rpc -32602: review_gate.exec.temperature=1.9 超出允许区间 [0.6, 1.5]"}
judge 0.8 -> {"code":500,"message":"rpc -32602: review_gate.judge.temperature=0.8 超出允许区间 [0.0, 0.2]"}
```

边界内正常接受：`guide 0.2` / `exec 1.5` / `judge 0.0` 均写入成功。

**与 §11.4 的建议对照：**

| §11.4 建议 | 实现 |
|---|---|
| 越界时 `config.set` 直接拒绝 | ✅ |
| 错误里带上合法区间 | ✅ `超出允许区间 [0.2, 0.5]` |
| 关闭状态下也应校验 | ✅ 已实现（我特意在 disabled 下测的） |

---

## 4. P2 验证：配置优先级与生效值

```
warn: --provider 覆盖 config.json 的 provider-mock（实际使用 provider-ollama）
warn: --model 覆盖 config.json 的 qwen2.5:7b（实际使用 qwen3.5:4b）
warn: --base-url 覆盖 config.json 的 http://127.0.0.1:11434（实际使用 http://127.0.0.1:11499）

featheragent 1.2.1-rc.2 starting  socket=...  provider=provider-ollama  codec=msgpack
effective model=qwen3.5:4b provider=provider-ollama base_url=http://127.0.0.1:11499
          (model_source=cli, provider_source=cli, base_source=cli)
```

**比要求做得更细**：除了三条冲突 warn，还单独打了一行 `effective ...` 并标注 `*_source=cli`。

> 这条对我这种"用脚本反复切 provider/model"的测试场景价值极高——rc.1 就是因为在这一点上静默，
> 我误判了一次功能回归。

---

## 5. ★ review-gate 真实模型实测（本轮核心数据）

### 5.1 全 4B（默认配置，即 `guide/exec/judge` 都跟随全局 `qwen3.5:4b`）

**3 次尝试，3 次 degraded：**

| # | 结果 | 原因 |
|---|---|---|
| 1 | `degraded` | `ollama 返回空响应（content 与 tool_calls 均为空）` |
| 2 | `degraded` | `rpc -32000: 验收项 2 缺少 criterion/check` |
| 3 | `degraded` | `ollama 返回空响应（content 与 tool_calls 均为空）` |

```
chat.finished {"review":{"approved":false,"degraded":true,"enabled":true,"rounds":0},"status":"ok"}
```

**两种降级原因，恰好命中 §11.2 预言的两个风险：**

- **"验收项缺少字段" = R1（rubric 结构退化成空话）**：4B 写不出合规的 `{criterion, check}`。
- **"空响应" = 模型抖动**：普通对话有 `model.retry`，但 review 路径上似乎没有重试，直接降级。

### 5.2 7B 指导/审批 + 4B 执行（按 §11.7 的 C 组思路）

```json
"guide": {"model":"qwen2.5-coder:7b","temperature":0.3}
"judge": {"model":"qwen2.5-coder:7b","temperature":0.1}
"exec":  {"model":"qwen3.5:4b","temperature":0.7}
```

**这次跑满了，但永远不通过：**

```
review.guide   {count:3, model:"qwen2.5-coder:7b", temperature:0.3, tokens_approx:109}

[exec 第1轮] → model.message "1. 操作系统是管理计算机硬件…"
review.verdict {pass:false, round:1, failed:["c2","c3"], evidence:"总结"}
review.feedback{failed:["c2","c3"], next_temperature:0.85,
  text:"…- c2: 无证据（按不通过处理）\n- c3: 无证据（按不通过处理）…"}

[exec 第2轮] → 加了小标题
review.verdict {pass:false, round:2, failed:["c2","c3"], evidence:"总结"}
review.feedback{next_temperature:1.0, text:"…c2/c3 无证据…"}

[exec 第3轮] → 补了"（证据：技术定义）"、"（证据：行业共识）"
review.verdict {pass:false, round:3, failed:["c2","c3"], evidence:"总结"}

review.finished{rounds:3, status:"unapproved",
  flag:"未经批准：已达最大重试轮次，返回最后一版", failed:["c2","c3"]}
chat.finished {"review":{"approved":false,"degraded":false,"rounds":3},"status":"ok"}
```

### 5.3 机制层面：全部正确

| 设计点 | 实现 |
|---|---|
| 三角色分离、各自 model/temperature | ✅ 事件里能看到 `model:"qwen2.5-coder:7b"`、`temperature:0.3` |
| 逐条判定 + evidence，不给总分 | ✅ `failed:["c2","c3"]` + `evidence` |
| 回注失败项 | ✅ `review.feedback.text` 明确列出 c2/c3 |
| 逐轮提温 | ✅ 0.85 → 1.0 |
| **超轮次兜底** | ✅ `flag:"未经批准：已达最大重试轮次，返回最后一版"`（`on_exhausted` 落地） |
| 降级不崩 | ✅ 空响应/格式错都转成 `degraded`，会话仍可用 |

**这部分实现质量很高**，§11 提的六条机制全部到位。

### 5.4 效果层面：**当前配置下没有价值**

| 维度 | 实测 |
|---|---|
| 单轮成本 | guide 1 次 + exec 3 次 + judge 3 次 ≈ **7 次模型调用**（普通对话 1 次） |
| 最终结果 | `unapproved`（3 轮全否） |
| 用户拿到的东西 | 与普通对话相同的答案 + 一个"未经批准"标签 |
| 净收益 | **负**（多花 6 倍算力，答案没变好） |

**卡点定位**（从数据看得很清楚）：

1. **`review.verdict` 的 `evidence` 字段反复是 `"总结"`** —— 两个字的垃圾值，说明 judge 没有真正"举证"；
2. **judge 判 c2/c3 "无证据"**，而 exec 第 3 轮明明写了"（证据：技术定义）"——说明 **judge 看的 `check` 描述与 exec 理解的验收点不是一回事**；
3. rubric 的 `check` 由 guide 自由生成，**judge 与 exec 对同一条 `check` 的理解不一致**，是本轮失败的核心。

### 5.5 建议（按性价比排序）

| 优先 | 建议 | 理由 |
|---|---|---|
| **1** | **rubric 的 `check` 必须是"字面可匹配"的判据**，如 `check:"答案中必须出现『调度』二字"`；judge 强制引用 `check` 原文再判定 | 5.4 的根因；现在 `check` 是自由文本，三方理解不一致 |
| **2** | **judge 的 `evidence` 为空或过短（如 <10 字）时，视为 judge 失败并重试 judge**，不要直接判 exec 不通过 | 本例 `evidence:"总结"` 就是典型的"judge 没干活" |
| **3** | **rubric 校验失败应重试 guide（1 次）**，而不是立刻 degraded | 5.1 的第 2 次就是这样白跑一整轮 |
| **4** | **review 路径复用 `model.retry`**（空响应自动重试 1 次） | 5.1 的第 1、3 次 |
| **5** | 默认 `guide`/`judge` 若跟随 4B，建议**在设置页标注"4B 下可能无法产出合规 rubric"**；或干脆默认把 guide/judge 指到 ≥7B | 5.1 的结论：全 4B 下 3/3 降级 |

---

## 6. S3 与门禁

### 6.1 S3（真实 `qwen3.5:4b`）

```
触发 5/5 · 工具名 5/5 · 执行成功 4/5 · 预期失败 1 · 真失败 0/5 · 答复 4/5
写入校验 workspace/w1.txt : 存在 -> 'HELLO_W_99'
```

**满分回归**（rc.1 因 P0 完全无法测）。

### 6.2 bench（7/7 全绿）

```
gates: assemble ✅ binary ✅ boot_lt_100ms ✅ cold_spawn_lt_250ms ✅
       rpc ✅ rss ✅ write_p95 ✅        （共 7 项，比 rc.1 多一个 cold_spawn）

binary 2.077 MB | warm boot p50 45.30 ms | cold spawn p50 173.25 ms
RSS 10,496 KB | write P95 3,751 µs | RPC P95 0.215 ms
```

R1 保持关闭，且 `cold_spawn` 已成为正式门禁项。

---

## 7. 问题清单

| # | 严重度 | 问题 | 建议 |
|---|---|---|---|
| **G1** | 🟠 | **review-gate 在真实本地模型上完全不产出"批准"结果**：全 4B → 3/3 降级；7B 指导/审批 → 3 轮全否 | 见 §5.5 五条，优先修 1、2 |
| G2 | 🟡 | `review.verdict.evidence` 出现 `"总结"` 这类无意义短值，却仍被采信 | evidence 过短/为空 → 视为 judge 失败 |
| G3 | 🟡 | `chat.finished.status:"ok"` 但 `review.approved:false`（或被 degraded） | status 建议反映真实结果（`ok` / `degraded` / `unapproved`） |
| — | ✅ | ~~P0 空 body~~ | **已修**，抓包实证 |
| — | ✅ | ~~P1 温度区间~~ | **已修**，文案与区间齐备 |
| — | ✅ | ~~P2 配置优先级~~ | **已修**，含 `effective` + `*_source` |

> **无阻断级缺陷**。rc.2 是一次干净的修复版；剩下的是 review-gate **效果**问题，不是**机制**问题。

---

## 8. 未验证项

| 项 | 说明 |
|---|---|
| review-gate 在更强模型（14B+）或云端模型下的表现 | 本机最大只有 7B |
| 五个 OpenAI 兼容 provider 实连 | 需本地 llama.cpp / NInfer / vLLM 服务器 |
| Bearer API Key 头注入 | 需可观测代理 |
| WebUI 设置页 / TUI Markdown / 网络盘 / 24h / 高 DPI | 未做 |

---

## 9. 复现命令

```powershell
cd FeatherAgent-1.2.1-rc.2-windows-x64
$env:HTTP_PROXY="http://127.0.0.1:34686"; Remove-Item Env:NO_PROXY -ErrorAction SilentlyContinue

# P0：抓包验证 body 是否正常（echo 服务见 §2）
python _scratch\echo_srv.py
.\featheragent.exe serve --root . --provider provider-ollama --model qwen3.5:4b --base-url http://127.0.0.1:11499
.\featheragent.exe chat --root . --session e --text "只回答 OK"

# P1：温度区间（应报错并带区间）
# POST /rpc {"method":"config.set","params":{"path":"review_gate.judge.temperature","value":0.8}}

# review-gate 真实模型
# config.json: review_gate.enabled=true, guide/judge.model="qwen2.5-coder:7b"
.\featheragent.exe chat --root . --session rv --text "用三句话说明什么是操作系统"

# S3 / 门禁
python tools\s3_batch.py
.\featheragent.exe bench --root . --json
```

---

## 10. 演进总览

| 指标 | rc.1 | rc.1（换包） | **1.2.1-rc.1** | **1.2.1-rc.2** |
|---|---|---|---|---|
| 内核体积 | 2.04 MB | 2.04 | 2.07 | **2.08 MB** |
| 热启动 | 92~175 | 92~175 | 41.9~45.8 | **45.3 ms** |
| 门禁 | 抖动 | 抖动 | 6/6 | **7/7** |
| 真实模型 | ✅ | ✅ | 🔴 P0 不可用 | **✅ 恢复** |
| S3 | 4/5 | 满分 | ⛔ 无法测 | **满分** |
| review-gate | — | — | mock 通过 | **机制 ✅ / 效果 ❌** |
| 阻断级缺陷 | 0 | 0 | 1 | **0** |

> **发版判断**：rc.2 作为**"修复 rc.1 回归"的补丁版**是合格的——P0/P1/P2 全部落地，无阻断缺陷。
>
> 但 **review-gate 若作为卖点，需要先解决"永远不批准"的问题**。
> 当前状态下它会：① 吃掉 6~7 倍算力；② 给出"未经批准"；③ 答案质量并无提升。
> 建议要么按 §5.5 修 judge/rubric 的对齐，要么**默认关闭**并标注"实验特性"。
