# FeatherAgent v1.2.1-rc.4.1 · Windows 真机复测报告

> 测试机：Windows x64 · Ollama 0.32.9 · qwen3.5:4b(exec) / qwen2.5-coder:7b(guide, judge)
> 复测时间：2026-11-25
> 被测包：`FeatherAgent-1.2.1-rc.4.1-windows-x64`
> 对照：`Windows真机复测报告-v1.2.1-rc.4.md`（N10 ～ N14）
> **本轮新增专题**：小模型在 FeatherAgent 环境 vs 裸跑的能力对比（代码 / 设计 / 翻译 / 答题）

---

## 1. 结论

| 项 | 结果 |
|---|---|
| **N10 路径长度** | ✅ **已修** —— `workspace/notes.txt` 被接受，guide 一次成功，文件真写入 |
| **N13 rubric-final** | ✅ **已修** —— `<ms>-rubric-final.json` 生成，`review.guide` 带 `final_artifact` |
| **N12 web-artifacts** | ✅ **已修** —— `data/web-artifacts/<session>/<ms>-<hash>-web-search.json` |
| **N11 正文工具调用** | ✅ **已修**（`tool_call <name> <json>` 可恢复）；⚠️ **XML `<function>` 风格未覆盖** |
| **N5 ～ N9 回归** | ✅ 语义/门禁均保持；⚠️ N7 仍约 1/2 失败（诊断已结构化）、N9 新增 `boot_gate_trusted=False` |
| **★ 小模型能力对比** | **质量持平（15/16 vs 15/16）**，FA 总耗时 **49s vs 104s（快 2.1 倍）** |

---

## 2. N10：路径长度 ✅（原始输出）

任务：`请用工具把三条关于「内存管理」的要点写入 workspace/notes.txt，每条一行，以「- 」开头`

### 2.1 guide 一次成功，路径被接受

```json
review.guide {"artifact":"...\\data\\review-artifacts\\n10\\1795604976393-guide-a1.json",
              "attempts":1, "count":5, "final_artifact":"...\\1795604976417-rubric-final.json", ...}
```

对比 rc.4 的同一条任务：`parse_error: must_include 超过 12 字（19 字）`，三次重试全败 → degraded。
**现在 attempts=1 就通过，count=5 条判据。**

### 2.2 文件确实写入了（原文）

```
- 堆栈管理与动态内存分配（malloc/free）的核心区别在于：堆栈使用自动管理，而动态分配需要手动处理释放以避免泄漏
- 垃圾回收机制通过标记清除、引用计数或分代收集等算法自动化内存回收，常见于 Java/Python等高级语言运行时环境
- 预防内存泄漏的最佳实践包括及时检测异常路径下的资源清理、避免悬垂指针和循环依赖导致的不可达对象无法被 GC
```

### 2.3 顺带验证 N6（主观判据不计门禁）

```json
review.warn {"text":"验收项 c5 是主观判据（含『清晰』）：不计入门禁；需要时改成可计数/可核对的格式"}
review.warn {"text":"没有描述型判据：judge 不参与本轮，全部由确定性判据决定"}
review.warn {"text":"min_points 机器计数不足，按软判据接受（确定性判据，不交 judge）：〔c2〕0/3 个要点"}
```

三条 warn 分别覆盖：主观判据剔除、judge 不参与、min_points 软接受 —— **N5/N6 的语义都在正确工作**。

### 2.4 ⚠️ 但整轮仍 failed（XML 风格畸形）

```json
chat.finished {"error":"XML syntax error on line 4: element <function> closed by </parameter>",
               "session":"n10","status":"failed"}
```

**N11 修的是 `tool_call <name> <json>` 这种纯文本形式，没覆盖 XML 风格的 `<function>…</parameter>`。**
这类错误会让整轮 failed（尽管文件已成功写入）。

---

## 3. N13：rubric-final ✅（原始文件清单）

```
data/review-artifacts/n10/
  ├── 1795604976393-guide-a1.json        ← 原始 guide 输出
  └── 1795604976417-rubric-final.json    ← 被采纳的归一化 rubric（新增）
```

`review.guide` 里也加了 `final_artifact` 字段，与 `artifact`（原始）并列 —— **能直接区分"模型写了什么"与"内核采纳了什么"**，
这正是上一轮我建议的缺口。✅

---

## 4. N12：web-artifacts ✅（原始路径）

```
data/web-artifacts/srch41/1795605054791-cd50490a-web-search.json
```

```json
"audit_artifact":"...\\data\\web-artifacts\\srch41\\1795605054791-cd50490a-web-search.json"
```

搜索事件流：`tool.call ×3`、`tool.result ×2`。**搜索结果现在可离线审计**，不必再解 msgpack 会话日志。✅

---

## 5. N11：正文工具调用恢复 ✅（含一次反例）

### 5.1 恢复成功（两次）

| # | status | tool.call | 答案里是否混入 `tool_call` 文本 |
|---|---|---|---|
| 1 | ok | `['tool.fs.read']` | **否** ✅ |
| 2 | ok | `['tool.fs.read','tool.shell','tool.fs.read']` | **否** ✅ |

第 2 次答案原文：

```
workspace/notes.txt 的第一行是：
> - 堆栈管理与动态内存分配（malloc/free）的核心区别在于：堆栈使用自动管理，而动态分配需要手动处理释放以避免
```

**rc.4 那种"把 `tool_call tool.fs.write {...}` 打进正文"的现象没有再出现。** ✅

### 5.2 ⚠️ 但 XML 风格仍会导致整轮失败

见 §2.4。**建议**：把 `<function>…</parameter>` 这类 XML 变体也纳入 N11 的格式恢复（或至少在失败前给一次"请用 JSON 工具调用格式"的纠正）。

---

## 6. ★ 新增专题：小模型能力对比（FeatherAgent 环境 vs 裸跑）

### 6.1 方法

- **裸跑**：直接 POST `http://127.0.0.1:11434/api/chat`，`messages=[{role:user, content:任务}]`，temperature 0.7，`stream:false`
- **FA**：`featheragent chat --allow tool.fs.write,tool.fs.read --text <同一任务>`
- 同一模型 `qwen3.5:4b`；每类 4 分制（规则化打分，见下）

### 6.2 结果

| 类目 | 裸跑分 | FA 分 | 裸跑耗时 | FA 耗时 | 裸跑长度 | FA 长度 |
|---|---|---|---|---|---|---|
| **代码** | 3 | **4** | 34.7s | **9.0s** | 543 | 934 |
| **设计** | 4 | 4 | 23.0s | 26.0s | 3775 | 3082 |
| **翻译** | 4 | 4 | 24.5s | **5.9s** | 126 | 91 |
| **答题** | 4 | 3 | 22.1s | **8.1s** | 201 | 167 |
| **合计** | **15/16** | **15/16** | **104s** | **49s** | — | — |

### 6.3 逐项原文对照

**代码（FA 胜 4:3）**

```
裸跑 (3/4, 543 字)：
```python
import re
def is_palindrome(s):
    """忽略大小写和非字母字符判断回文"""
    cleaned = ''.join(c.lower() for c in s if c.isalpha())
    …

FA (4/4, 934 字)：
```python
def is_palindrome(s: str) -> bool:
    """检查字符串是否为回文（忽略大小写和非字母字符）"""
    filtered = ''.join(char.lower() for char in s if char.isalpha())
    …
```

差异点：FA 版**带类型标注** `(s: str) -> bool`，且测试用例更完整（命中 `assert/>>>/is_palindrome(` ≥3 次）。

**设计（持平 4:4，裸跑更长）**

```
裸跑 3775 字：「以下是一个面向生产环境的高性能、可扩展的短链接服务的推荐数据库表结构设计方案…」
FA   3082 字：「# 短链接服务数据库表结构  ## 📋 核心表：short_url  | 字段名 | 类型 | 约束/说明 | …」
```

裸跑更"铺陈"（先讲业务场景再给表），FA 更直接（先给表）。**都能覆盖表/字段/索引**。

**翻译（持平 4:4，FA 更快更短）**

```
裸跑：Local small models can also perform daily question-and-answer and code completion tasks
      even on resource-constrained hardware.
FA ：Local small models can also complete daily Q&A and code generation on constrained hardware.
```

**答题（裸跑胜 4:3）**

```
裸跑：1. **连接方式不同**：TCP 是面向连接的协议（通信前需建立连接），而 UDP 是无连接协议。
      2. **可靠性与顺序性不同**：TCP 保证数据的可靠传输和有序到达…
      3. **控制机制与开销不同**：TCP …

FA ：1. **连接性** — TCP是面向连接的协议（握手-数据传输-挥手），UDP是无连接的协议
      2. **可靠性** — TCP提供可靠交付（重传、确认机制），UDP只提供尽力而为…
      3. **传输方式** — TCP按字节流传…
```

**FA 版少了"有序"这个关键词**（第 3 点讲"字节流"而非"有序/开销"）→ 关键词打分少 1 分。属覆盖面略窄，不是错误。

### 6.4 分析与结论

| 观察 | 解释 |
|---|---|
| **质量基本持平**（15 分 vs 15 分） | FA 的系统提示词（"回答简洁、工具优先"）没有损害通用能力 |
| **代码类 FA 更好** | 系统提示词里的"结构化输出"倾向让 4B 加了类型标注和更完整测试 |
| **答题类 FA 略窄** | 简洁倾向导致第三点覆盖不足——**简洁与完整存在张力** |
| **FA 总耗时快 2.1 倍** | 推测是 **固定系统提示词前缀被 Ollama KV 缓存复用**（裸跑每次全量 prefill）。设计类因输出长（3k+ 字）反而更慢，符合"生成时间主导"的解释 |
| **输出长度** | 代码 FA 更长、设计/翻译/答题 FA 更短 → 提示词确实在"按类型调节长度" |

> **对主人的结论**：**在 4B 上，FeatherAgent 环境与裸跑质量等价，且响应更快**。
> 唯一需要留意的是"简洁倾向"会让答题类答案覆盖面变窄——如果用户要"穷举式回答"，可能需要显式要求。

**打分方法的局限（如实说明）**：本轮用关键词/结构规则打分（是否含 `def`/`return`、是否提及索引、ASCII 占比、关键词命中），
**只能反映"覆盖面"，不能反映"正确性"**。后续若要更严格，建议加：代码用 `py_compile` 实测、翻译用回译比对、答题用人工抽查。

---

## 7. N5 ～ N9 回归

### 7.1 N7：错误已结构化，但失败率仍约 1/2

第 1 次的原始输出（**不再是一句 `os error 2`**）：

```json
{"code":"artifact_missing",
 "error":"[bench:rpc_connect] C:\\Users\\<user>\\AppData\\Local\\Temp\\fa-bench-16336-1e00cbfe8301\\rpc\\runtime\\ipc\\featheragent.sock: 系统找不到指定的文件。 (o…"}
```

- ✅ `code: artifact_missing`
- ✅ 操作名 `[bench:rpc_connect]`
- ✅ 完整路径，且是**唯一临时目录** `fa-bench-16336-1e00cbfe8301`（N7 的修复生效）

**但底层故障仍在**：bench 自己起的临时内核没有产出 socket。2 次里 1 次失败。
**建议**：rpc_connect 加"等待 socket 出现"的轮询（现在似乎是固定等待），或把该子进程的 stdout 附在错误里。

### 7.2 N9：`boot_gate_trusted` 又变成红项

第 2 次的完整 gates：

```
assemble_p95_lt_30ms = True      binary_le_8mb = True
boot_gate_raw = True             boot_gate_status = advisory
boot_gate_trusted = False   ← 红
boot_limit_ms = 100.0            boot_lt_100ms = True
boot_mode = warm_in_process_same_install_root
cold_spawn_limit_ms = 250.0      cold_spawn_lt_250ms = True
cold_spawn_mode = real_process_spawn
metrics_consistent = True        reliability = low
rpc_p95_lt_1ms = True            rss_le_15mb = True     write_limit_us = 10000.0
```

**统计上 9/10，唯一"失败"是 `boot_gate_trusted`（因为 `reliability=low`）。**

这条设计上说得通（"读数不可信"本身是个信号），但**它和性能门禁混在同一个 pass/fail 计数里**，
会让"机器繁忙"看起来像"版本回归"——这正是 N9 想解决的问题。

**建议**：把 `boot_gate_trusted` 归入 `advisory_gates` 单独列出，不参与 `passed/total`。

### 7.3 N2 / N5 / N6 / N8

- `metrics_consistent = True`、`P50=45.8 / P90=89.8`（P50≤P90）→ **N2 保持**
- `match=all/min` + `{hits}/{total}` → **N5 保持**（见 §2.3）
- 主观判据剔除 + 禁词约束 → **N6 保持**（见 §2.3）
- `tool.call` 带 `attempt`/`call_index` → **N8 保持**

---

## 8. 问题清单

| # | 严重度 | 问题 | 建议 |
|---|---|---|---|
| **N15** | 🟠 | **XML 风格畸形工具调用（`<function>…</parameter>`）未被 N11 覆盖**，导致整轮 failed（文件已写入却报失败） | 把 XML 变体纳入格式恢复；或在失败前给一次"请用 JSON 工具调用格式"的纠正 |
| **N16** | 🟠 | **bench 仍约 1/2 失败**：临时内核未产出 socket（诊断已结构化，根因未除） | rpc_connect 改为轮询等待；错误里附子进程 stdout |
| **N17** | 🟡 | `boot_gate_trusted=False` 混在常规门禁计数里，机器繁忙时显示为"失败" | 归入 `advisory_gates`，不参与 `passed/total` |
| **N18** | 🟡 | FA 的"简洁倾向"让答题类覆盖面变窄（少"有序/开销"） | 系统提示词里区分"简洁"与"穷举"，或对列举类任务显式要求"覆盖全部要点" |
| — | ✅ | ~~N10 路径 12 字~~ | **已修**（64 字，`notes.txt` 通过） |
| — | ✅ | ~~N11 正文 tool_call~~ | **已修**（纯文本形式；见 N15 的 XML 缺口） |
| — | ✅ | ~~N12 搜索落盘~~ | **已修**（`web-artifacts`） |
| — | ✅ | ~~N13 rubric-final~~ | **已修**（含 `final_artifact`） |
| — | ✅ | ~~N5/N6/N8/N9 语义~~ | 保持 |

---

## 9. 未验证项

| 项 | 说明 |
|---|---|
| 附件选择器 / PTY 流程 | 未做 |
| `-en` 前端实际界面、策略下拉、权限弹窗、设置页字段 | 未做 |
| 五 provider 实连、Bearer、网络盘、24h、高 DPI | 未做 |
| 能力对比的**正确性**校验（编译/回译/人工） | 本轮只做覆盖面规则打分 |

---

## 10. 复现命令

```powershell
cd FeatherAgent-1.2.1-rc.4.1-windows-x64
$env:HTTP_PROXY="http://127.0.0.1:34686"; Remove-Item Env:NO_PROXY -ErrorAction SilentlyContinue
.\featheragent.exe serve --root . --provider provider-ollama --model qwen3.5:4b
.\featheragent-webui.exe --root .
# config.set: guide/judge.model=qwen2.5-coder:7b, review_gate.enabled=true

# N10 + N13
.\featheragent.exe chat --root . --session n10 --allow tool.fs.write,tool.fs.read `
  --text "请用工具把三条关于「内存管理」的要点写入 workspace/notes.txt，每条一行，以「- 」开头"
Get-ChildItem .\data\review-artifacts\n10

# N12：搜索落盘
.\featheragent.exe chat --root . --session s1 --text "搜索 Rust ownership 的两条要点"
Get-ChildItem .\data\web-artifacts\s1

# 能力对比：裸跑
curl -s -X POST http://127.0.0.1:11434/api/chat -H "Content-Type: application/json" `
  -d '{"model":"qwen3.5:4b","stream":false,"messages":[{"role":"user","content":"TCP 和 UDP 的三个主要区别"}]}'
# 能力对比：FA
.\featheragent.exe chat --root . --session cap --text "TCP 和 UDP 的三个主要区别"
```

---

## 11. 演进总览

| 指标 | rc.3.6 | rc.4 | **rc.4.1** |
|---|---|---|---|
| 路径 must_include | ❌ 12 字拦死 | ❌ | **✅ 64 字** |
| 正文 tool_call 恢复 | ❌ | ❌ | **✅（纯文本）；XML 仍缺** |
| 搜索落盘 | 无 | 无 | **✅ web-artifacts** |
| rubric-final | 无 | 无 | **✅** |
| bench 错误诊断 | 一句 os error 2 | 一句 | **✅ 结构化 + 临时目录** |
| bench 成功率 | 6/8 | 2/2 | **1/2（根因未除）** |
| 门禁 | advisory | advisory | **+ boot_gate_trusted** |
| 多任务 ON | 0/4 | 0/4 | **N10 已修，路径类任务可过** |
| 能力对比（vs 裸跑） | — | — | **15:15（质量持平）/ 快 2.1×** |

> **这版把上一轮报的四个缺口全补上了**（路径、正文工具调用、搜索落盘、rubric 最终版），
> 而且补得很干净——每一条都有可核对的产物或事件字段。
>
> 剩下的 N15/N16 都是"边界格式/子进程稳定性"，N17/N18 是"呈现与提示词调优"，
> 没有阻断级问题。**从"能不能跑通"进入"跑得稳不稳、呈现好不好"的阶段了。**
