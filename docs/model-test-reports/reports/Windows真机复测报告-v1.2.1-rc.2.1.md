# FeatherAgent v1.2.1-rc.2.1 · Windows 真机复测报告

> 测试机：Windows x64（同前）· Ollama 0.32.9
> 复测时间：2026-11-24
> 被测包：`FeatherAgent-1.2.1-rc.2.1-windows-x64`
> 对照：`Windows真机复测报告-v1.2.1-rc.2.md`（G1/G2/G3）

---

## 1. 结论

**G1（review-gate 永不批准）已解决：真实模型下能走到 `status:"approved"`。**
**G2（evidence 过短）只解决了一半：长度够了，但 judge 用"回显提示词"满足长度要求。**
**G3（status 语义）已修：`ok` / `unapproved` / `degraded` 三态 + `review.status` + 归一化 `items[]`。**

**但引出一个新的、更根本的问题：`must_include` 用了整句，门禁退化成"你有没有抄我写的句子"——而指导者事实上把答案写好了。**

| 项 | rc.2 | rc.2.1 | 证据 |
|---|---|---|---|
| G1 review-gate 不批准 | ❌ 3/3 不通过 | ✅ **能 approved** | 7B guide/judge + 4B exec → `rounds:3, status:"approved"` |
| G2 evidence 过短 | ❌ `"总结"` | ⚠️ **长度达标但内容回显** | `evidence:"逐条核对结论，至少 10 字…"`（提示词原文） |
| G3 status 语义 | ❌ 恒 `ok` | ✅ **已修** | `ok` / `unapproved` / `degraded` + `review.status` |
| R-03 must_include 硬检查 | — | ✅ **实现，但判据设计有隐患** | 见 §3 |
| bench 门禁 | 7/7 | ✅ **7/7** | 热 60.33 ms / 冷 179.72 ms |
| S3 | 满分 | ⚠️ **3/5**（本轮样本） | 见 §5 |

---

## 2. G1/G3 验证：review-gate 真的能批准了

### 2.1 配置

```
review_gate = {
  "enabled": true,
  "guide":  {"model": "qwen2.5-coder:7b", "temperature": 0.3},
  "judge":  {"model": "qwen2.5-coder:7b", "temperature": 0.1},
  "exec":   {"model": "qwen3.5:4b",        "temperature": 0.7},
  "max_rounds": 2, "on_exhausted": "return_last_with_flag"
}
```

### 2.2 事件流（第一次跑）

```
review.guide  {attempts:1, count:3, model:"qwen2.5-coder:7b", temperature:0.3, tokens_approx:130}
review.verdict{pass:false, round:1, failed:["c1","c2","c3"],
               evidence:"答案未逐字出现必须短语『操作系统是管理计算机硬件和软件资源的系统。』",
               items:[{id, criterion, check, evidence, pass}, …]}
review.feedback{failed:[...], next_temperature:0.85,
               text:"- c1: 判据=「可核对的要求」；核对=逐字核对 must_include 是否出现；未通过证据=…"}
review.verdict{pass:true, failed:[], items:[{pass:true}…]}
review.finished{rounds:2, status:"approved", failed:[]}
chat.finished {status:"ok", review:{approved:true, degraded:false, enabled:true, rounds:2, status:"approved"}}
```

### 2.3 对照 §11 / rc.2 建议

| 建议 | 实现 |
|---|---|
| rubric 要求字面 `must_include`，**judge 之前先硬检查** | ✅ verdict 的 evidence 直接是 `答案未逐字出现必须短语『…』`（规则判定，非 LLM 判断） |
| feedback 带 `criterion` + `check` | ✅ `判据=「…」；核对=…；未通过证据=…` |
| 归一化 `items[]` | ✅ `{id, criterion, check, evidence, pass}` 逐条 |
| `chat.finished.status` 三态 | ✅ 本轮见到 `ok`（配合 `review.status:"approved"`） |
| guide 重试计数可观测 | ✅ 新增 `attempts` 字段 |

**G1 与 G3 关闭。** 这是本轮最实质的进步：从"永远不批准"变成"能在 2~3 轮内批准"。

---

## 3. 🔴 新问题 H1：`must_include` 用整句 → 门禁退化成"抄句子"

### 3.1 证据

**指导者产出的三条判据**（从 feedback 反推）：

```
c1: must_include = 「操作系统是管理计算机硬件和软件资源的系统。」
c2: must_include = 「它提供用户界面和程序开发接口。」
c3: must_include = 「常见的操作系统类型包括Windows、macOS和Linux。」
```

**执行者的最终答案**（逐字核对，三条全中）：

```
1. **操作系统是管理计算机硬件和软件资源的系统。**
2. **操作系统的主要作用包括进程管理、内存管理、文件系统管理、设备驱动和用户界面。**
3. **常见的操作系统类型包括Windows、macOS和Linux。**
```

我逐条比对：**三个 `must_include` 全部逐字出现** ✅（硬检查是真生效的，这是好事）。

### 3.2 但这意味着什么

| 环节 | 实际发生的事 |
|---|---|
| 指导者（7B） | 写了 3 条**完整句子**，每条就是答案的一句话 |
| 执行者（4B） | 把这 3 句抄进去，加粗编号 |
| 硬检查 | 子串匹配 → 全过 |
| 审批者 | 无话可说 → 批准 |

**代价**：guide 1 次 + exec 3 次 + judge 3 次 ≈ **7 次模型调用**
**产出**：一段"由指导者撰写、执行者抄写"的三句话答案

这块质量门**没有在衡量答案质量，它在衡量"是否照抄指导者的句子"**。
更微妙的是：**指导者事实上把答案写好了，执行者退化成了复读机。**

> 这是典型的 Goodhart's law：指标（must_include 命中率）变成了目标本身。
> rc.2 的问题是"judge 太虚 → 永不通过"；rc.2.1 的问题相反——"判据太硬 → 通过得太廉价"。

### 3.3 建议

| 优先 | 建议 |
|---|---|
| **1** | **`must_include` 限制长度与数量**：单条 ≤20 字、总数 ≤5 条，且**禁止整句**（含句号结尾的完整句子应被拒绝）。判据应该是"关键词/短语"，不是"结论句" |
| **2** | **增加第二种判据类型**：除 `must_include` 外，支持 `must_not_include`（禁词）、`min_points`（至少 N 个要点）、`pattern`（正则）。只用子串会造成"抄句子即通过" |
| **3** | **指导者不应写出答案内容**：guide 的 prompt 里明确"给出**可核对的判据**，不要给出预期答案原文"。现在 guide 直接把答案写成句子，等于抢了 exec 的活 |
| **4** | 若坚持用整句判据，则**必须同时考核"exec 是否在无提示下也能产出"**（如再跑一次不带 rubric 的对照），否则质量门是自欺 |

---

## 4. 🟡 G2 只解决了一半：judge 用"回显提示词"凑长度

第三次 verdict 的字段：

```json
"evidence": "逐条核对结论，至少 10 字。每条 evidence 必须引用答案原文并至少 10 字；
             禁止\"总结\"\"通过\"\"ok\"\"是\"等占位词。顶层 evidence 也至少 10 字。"
"items": [{"evidence": "引用答案中的原文至少 10 字", "pass": true}, …]
```

**这是 prompt 原文的回显，不是对答案的引用。**

所以 `evidence ≥ 10 字` 的规则被绕过了：judge 只要把提示词抄一遍就达标。
rc.2 的问题是 `"总结"`（2 字），rc.2.1 变成"长度够了但内容是提示词"。

**建议**：
1. **evidence 必须能在答案里找到连续 ≥10 字的子串**（做一次子串校验，找不到就判 judge 失败）；
2. 或直接**取消 judge 的自由文本判据**，让"是否通过"完全由规则层（must_include / must_not_include / pattern）决定，judge 只负责**生成判据的补充说明**——这样反而更可靠；
3. 加一条"evidence 与 judge prompt 的重合度"检查，重合度过高即判无效。

---

## 5. S3 回归（本轮样本 3/5）

```
触发 4/5 · 工具名命中 3/5 · 执行成功 3/5 · 预期失败 1 · 真失败 1/5 · 答复 3/5
写入校验 workspace/w1.txt : 存在 -> 'HELLO_W_99'   ← 写盘仍正常
```

两个异常：

| 任务 | 现象 |
|---|---|
| t5（读+总结） | **(无) 工具未触发**，直接作答 |
| t6（读不存在文件） | 出现 **`tool.fs`** 这个畸形工具名（连同 `tool.shell`×2 一起调用） |

`tool.fs` 不是已注册工具，内核按业务错误处理了，未熔断。**属模型侧的格式幻觉**（4B 在长 prompt 下偶发），
但对 5 个任务的小样本来说，这个抖动足以让分数从 4/5 掉到 3/5。

> 建议：`s3_batch.py` 对 t5/t6 这类任务加"必须先调用工具"的硬提示；
> 同时建议内核在工具名不匹配时回一条**明确的可纠错提示**（如 `未知工具 tool.fs，可用：tool.fs.read/tool.fs.write/…`），
> 让模型能自我纠正而不是继续幻觉。

---

## 6. bench（7/7 全绿）

```
gates: assemble ✅ binary ✅ boot_lt_100ms ✅ cold_spawn_lt_250ms ✅
       rpc ✅ rss ✅ write_p95 ✅

binary 2.081 MB | warm boot 60.33 ms | cold spawn 179.72 ms
RSS 10,480 KB | write P95 4,128 µs | RPC P95 0.223 ms
```

热启动从 rc.2 的 45.3 ms 升到 60.33 ms（仍远低于 100 ms 门限），属正常波动。

---

## 7. 问题清单

| # | 严重度 | 问题 | 建议 |
|---|---|---|---|
| **H1** | 🟠 | `must_include` 用整句 → 门禁退化为"抄句子"，指导者实际写了答案 | 限长/禁整句；增加 `must_not_include`/`min_points`/`pattern` 判据类型 |
| **H2** | 🟠 | judge 用**回显提示词**满足 `evidence ≥10 字` | evidence 必须能在答案中找到连续 ≥10 字子串 |
| H3 | 🟡 | `tool.fs` 畸形工具名无纠错提示 | 返回"未知工具 + 可用列表" |
| H4 | 🟡 | t5 未触发工具、t6 多调 | 脚本加硬提示 |
| — | ✅ | ~~G1 永不批准~~ | **已修**（能 approved） |
| — | ✅ | ~~G3 status 语义~~ | **已修**（三态 + review.status） |
| — | ✅ | ~~P0/P1/P2（rc.2 项）~~ | 保持 |

---

## 8. 未验证项

| 项 | 说明 |
|---|---|
| TUI `/settings` 19 字段表单（R-02） | 需 PTY 逐项操作，本轮未做 |
| Ollama 原生模型发现（`/api/tags`） | 需在设置页点"刷新模型列表"，未做 |
| `unapproved` / `degraded` 两态的实拍 | 本轮只触发到 `approved` 与（未构造）超轮次 |
| 五个 OpenAI 兼容 provider 实连、Bearer 头注入 | 未做 |
| WebUI Markdown、网络盘、24h、高 DPI | 未做 |

---

## 9. 复现命令

```powershell
cd FeatherAgent-1.2.1-rc.2.1-windows-x64
$env:HTTP_PROXY="http://127.0.0.1:34686"; Remove-Item Env:NO_PROXY -ErrorAction SilentlyContinue

.\featheragent.exe serve --root . --provider provider-ollama --model qwen3.5:4b
.\featheragent-webui.exe --root .

# 经 /rpc 配 review-gate（guide/judge 用 7B）
# {"method":"config.set","params":{"path":"review_gate.guide.model","value":"qwen2.5-coder:7b"}}
# {"method":"config.set","params":{"path":"review_gate.judge.model","value":"qwen2.5-coder:7b"}}
# {"method":"config.set","params":{"path":"review_gate.enabled","value":true}}

.\featheragent.exe chat --root . --session rv --text "用三句话说明什么是操作系统"
# 观察 review.verdict.items[].evidence 是否为提示词回显
```

---

## 10. 演进总览

| 指标 | rc.1 | rc.2 | **rc.2.1** |
|---|---|---|---|
| 真实模型 | 🔴 P0 不可用 | ✅ | ✅ |
| review-gate | mock 通过 | ❌ 3/3 不通过 | **✅ 2~3 轮内 approved** |
| 判据质量 | — | judge 虚判 | **硬检查生效，但判据=整句** |
| status 语义 | 恒 ok | 恒 ok | **ok/unapproved/degraded** |
| bench | 6/6 | 7/7 | **7/7** |
| S3 | ⛔ | 满分 | 3/5（模型波动） |
| 阻断级缺陷 | 1 | 0 | **0** |

> **发版视角**：rc.2.1 把 rc.2 的三个问题都处理了，机制层面已经完备，无阻断缺陷。
>
> 但 review-gate 作为卖点要谨慎：它现在**能用、会批准、但衡量的是"抄得像不像"**。
> 如果对外宣传"三段校验提升质量"，建议先把 §3.3 的四条改掉——否则一旦用户发现
> "答案就是指导者写的那三句话"，这个功能的信任度会很难补。
>
> 折中方案：**默认关闭 + UI 标注「实验特性」**，同时在设置页保留 ≥7B 的推荐提示（这点已经做了）。
