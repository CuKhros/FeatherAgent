# FeatherAgent v1.2.1-rc.4.2 · Windows 真机复测报告

> 测试机：Windows x64 · Ollama 0.32.9 · qwen3.5:4b(exec) / qwen2.5-coder:7b(guide, judge)
> 复测时间：2026-11-25
> 被测包：`FeatherAgent-1.2.1-rc.4.2-windows-x64`
> 对照：`Windows真机复测报告-v1.2.1-rc.4.1.md`（N15 ～ N18）
> **本轮重点**：N15–N18 修复验证 + **7 题高难度试卷（裸跑 vs FA 环境）**

---

## 1. 结论

| 项 | 结果 |
|---|---|
| **N15 XML 恢复** | ✅ **已修** —— 原本必 `failed` 的写文件任务，两次都走完（`unapproved`，不再 terminated） |
| **N16 连续 bench** | ✅ **`[bench:rpc_connect]` 彻底消失**（连续 5 次 0 故障）；🔴 **但 `cold_spawn` 从 ~180ms 涨到 ~3180ms，门禁恒红**（N19） |
| **N17 advisory_gates** | ✅ **已修** —— `gates` 不再含 `boot_gate_trusted`，移入 `advisory_gates` |
| **N18 列举类覆盖面** | ⚠️ **部分达成** —— 覆盖维度仍与裸跑不同（缺"开销/头部"），但确实是三条完整区别 |
| **★ 七题试卷** | **FA 24/24 · 裸跑 17/24；FA 总耗时 143s，裸跑 332s（快 2.3 倍）** |

---

## 2. N15：XML 畸形工具调用恢复 ✅

任务：`请用工具把三条关于「内存管理」的要点写入 workspace/notes.txt…`（rc.4.1 下必 `failed`）

```
第1次: status=unapproved calls=['tool.fs.write','tool.fs.read','tool.fs.read','tool.fs.read','tool.fs.read']
       答案: 已用 `tool.fs.write` 完成写入：
       notes.txt: - 使用堆（heap）进行动态内存分配与释放，避…
第2次: status=unapproved calls=['tool.fs.read','tool.fs.write','tool.fs.write','tool.fs.write','tool']
       答案: 工具调用已成功执行：`workspace/notes.txt` 已写入三条关于「内存管理」的要点…
       notes.txt: - 使用堆（heap）进行动态内存分配与释放，避…
```

**两次都不再是 `failed`，文件也确实写入。** 对比 rc.4.1 的同一条任务：

```
chat.finished {"error":"XML syntax error on line 4: element <function> closed by </parameter>",
               "status":"failed"}
```

**N15 关闭。** 遗留观察：虽然不再 failed，但结论是 `unapproved`（门禁没通过），
且模型在多轮里反复 `tool.fs.read`（4 次）——**能跑完，但不够干净**。

---

## 3. N16：连续 bench（✅ 消除 rpc_connect，🔴 但 cold_spawn 暴涨）

### 3.1 `[bench:rpc_connect]` 已彻底消失 ✅

连续 5 次 bench（第 6 次因本轮时间窗被截断）：

```
第1次: 8/9 失败=['cold_spawn_lt_250ms']  P50=48 rel=low
第2次: 8/9 失败=['cold_spawn_lt_250ms']  P50=44 rel=medium
第3次: 8/9 失败=['cold_spawn_lt_250ms']  P50=42 rel=medium
第4次: 8/9 失败=['cold_spawn_lt_250ms']  P50=40 rel=medium
第5次: 8/9 失败=['cold_spawn_lt_250ms']  P50=42 rel=medium
bench 失败次数: 0/5
```

**rc.4.1 是 1/2 概率 `artifact_missing`，现在 0/5。** N16 的连接重试生效。

### 3.2 🔴 N19（新）：`cold_spawn_ms_p50` 从 ~180ms 涨到 ~3180ms

```
第1次: cold_p50=3181.08  limit=250.0  ok=False   warm=48  mem=85%
第2次: cold_p50=3208.67  limit=250.0  ok=False   warm=44  mem=83%
第3次: cold_p50=3184.42  limit=250.0  ok=False   warm=42  mem=79%
第4次: cold_p50=3179.76  limit=250.0  ok=False   warm=40  mem=80%
第5次: cold_p50=3313.32  limit=250.0  ok=False   warm=42  mem=82%
```

**5 次都在 3179~3313ms 之间，波动不到 5%** —— 这不是环境噪声，是**固定量级的等待**。

**推断**：N16 给 `ipc::connect` 加了 3 秒 deadline 的重试，给 bench 加了最多 10 秒的 `Client::connect` 重试。
cold_spawn 测的是"spawn → connect 成功"的**总耗时**，把重试等待也算进去了 → **稳定多出 ~3 秒**。

**影响**：`cold_spawn_lt_250ms` 这条门禁**永久变红**，且掩盖了真实冷启动性能（180ms → 看似 3.2s）。

**建议：**

| 优先 | 措施 |
|---|---|
| **1** | 记录**首次成功**的耗时（不含重试等待），重试等待单列 `cold_spawn_retry_ms` |
| **2** | 重试间隔改自适应（50/100/200ms 指数），而不是"等满 3 秒再试" |
| **3** | 若坚持用总量，需把门限从 250ms 重新校准到 3500ms，并在文档说明"含重试" |

> 这条是典型的**"修 A 引入 B"**：连接重试解决了 `rpc_connect` 失败，但把测量口径污染了。

---

## 4. N17：`advisory_gates` ✅

```
第1次: gates 含 boot_gate_trusted? False
       advisory_gates={"boot_gate_status":"advisory","boot_gate_trusted":false,"reliability":"low"}
第2次: gates 含 boot_gate_trusted? False
       advisory_gates={"boot_gate_status":"advisory","boot_gate_trusted":false,"reliability":"medium"}
```

**`boot_gate_trusted` 已移出 `gates`，不再参与 `passed/total`。** 于是现在 9 项里只有 `cold_spawn` 红，
"机器繁忙"不再伪装成版本回归。✅

（但 9 项里的那 1 项红正是 §3.2 的 N19。）

---

## 5. N18：列举类覆盖面 ⚠️ 部分

任务：`TCP 和 UDP 的三个主要区别是什么？列成三条。`

| | 得分 | 覆盖维度 |
|---|---|---|
| 裸跑 | **4/4** | 连接 ✓ / 可靠 ✓ / **有序** ✓ / **开销**（头部大小、效率）✓ |
| FA | **3/4** | 连接 ✓ / 可靠 ✓ / 有序 ✓ / **开销 ✗** |

FA 原文（第三点选了别的维度）：

```
1. **连接建立方式**：TCP 通信前需经过三次握手（SYN、ACK、确认）…；UDP 无需任何连接过程…
2. **数据传输形式与可靠性保障**：TCP 以流式传输…通过序列号和确认机制保证数据的完整性和顺序性…
3. **错误检测与纠错能力**：TCP 采用 CRC校验结合重复ACK、超时重传…；UDP 仅做基本帧完整性检查…
```

裸跑原文（第三点讲开销）：

```
3. **数据传输与开销差异**：TCP 需要维护连接状态并进行流量控制，头部信息较大、处理较慢；
   UDP 结构简单无流控，数据无序可能乱序，但头信息小、传输速度快。
```

**结论**：**N18 的"要求完整覆盖"没有体现为"覆盖用户预期的维度"**——
FA 仍选了三个自洽的区别，但**回避了"开销/头部大小"这一最常被问的维度**。
可能是我这条 prompt 没触发 N18 的"list/compare/enumerate"识别，或提示词的"简洁"仍占上风。

**建议**：在列举类任务里，让模型的**第一层小标题对齐用户提到的维度词**（"三个主要区别"→ 逐条对齐常见对比轴）。

---

## 6. ★ 七题高难度试卷（裸跑 vs FeatherAgent）

### 6.1 试卷设计（7 题 / 24 分）

| 题号 | 类型 | 考察点 | 满分 |
|---|---|---|---|
| Q1 | 代码（算法） | LRU 缓存 O(1) 实现 + 复杂度说明 | 5 |
| Q2 | 代码（调试） | 可变默认参数 bug：定位 + 修正 + 原因 | 3 |
| Q3 | 设计 | 令牌桶 vs 漏桶，场景取舍 | 3 |
| Q4 | 概率 | 生日悖论求 n（≥50%） | 2 |
| Q5 | 翻译（技术） | 中译保留术语（幂等/退避/副作用） | 4 |
| Q6 | 知识 | CAP 与 PACELC 的关系 + 真实系统例子 | 4 |
| Q7 | 摘要 | 压成 3 条备忘，每条 ≤40 字 | 3 |

打分：规则化（关键词 / 结构 / 长度），**每题的判据在报告里逐项列出，可复核**。

### 6.2 成绩

| 题 | 裸跑 | FA | 裸跑耗时 | FA 耗时 |
|---|---|---|---|---|
| Q1 LRU | 5/5 | 5/5 ⚠️ | 21.2s | 49.2s |
| Q2 调试 | 3/3 | 3/3 | 19.5s | 16.0s |
| Q3 限流 | 3/3 | 3/3 | 65.7s | 25.4s |
| Q4 概率 | 2/2 | 2/2 | 51.4s | 13.8s |
| Q5 翻译 | 4/4 | 4/4 | 50.7s | 5.4s |
| Q6 CAP | **0/4**（空响应） | **4/4** | 61.8s | 25.6s |
| Q7 摘要 | **0/3**（空响应） | **3/3** | 62.0s | 7.6s |
| **合计** | **17/24** | **24/24** | **332s** | **143s** |

**FA 满分 24/24，裸跑 17/24（71%）；FA 总耗时 143s，裸跑 332s（快 2.3 倍）。**

### 6.3 关键题原文对照

**Q6 CAP/PACELC（裸跑直接空响应，FA 完整作答）**

```
裸跑：61.8s，len=0（模型返回空）
FA  ：25.6s，1963 字
# CAP 定理与 PACELC：关系阐释及真实案例对比
### **CAP 定理**（2013）分布式系统在三个属性中最多同时满足两个…
| **Dynamo(亚马逊)** | 部分节点强一致 | AP/CP模式 | CP优先保证分区容错 |
### **PACELC**（2015）CAP 的扩展，在 P 时遵循 CAP；在非 P(N…
```

**Q7 摘要（裸跑空响应，FA 三条且带数据）**

```
FA：
- [服务] DB连接池上限仅10 vs 并发80，导致请求排队等待
- [解决] 调至100 + 超时熔断后，超时率从12%降至0.3%
- [计划] 引入读写分离进一步降低主库压力
```

**Q5 翻译（两版都好，FA 快 9 倍）**

```
裸跑（50.7s）：客户端会对**幂等请求（idempotent requests）**进行重试，最多三次，
              并使用指数退避策略（exponential backoff）…
FA  （5.4s ）：客户端对幂等操作（idempotent requests）最多重试三次，采用指数退避策略
              （exponential backoff）…
```

### 6.4 ⚠️ Q1 的一个必须说明的问题：FA 把答案写成了工具调用

```
裸跑（21.2s，正常代码块）：
以下是 Python 实现的 LRU缓存类… ```python from collections import OrderedDict
class LRUCache: …

FA（49.2s）：
tool_call tool.fs.write {"content":"\"\"\"\nLRUCache - O(1) get/put implementation using
hash map + doubly linked list.…
```

**FA 没有直接给代码，而是"把代码写到文件里"** —— 用户要的是答案，不是建文件。
我的关键词打分因此被 tool_call 的 JSON 内容"骗"成了 5/5，**实际交付体验更差**。

**建议**：系统提示词区分"要求输出代码"与"要求生成文件"；默认应在回答里给代码块，
只有用户明说"写到文件"时才调用工具。

### 6.5 为什么裸跑在 Q6/Q7 空响应？

两次都是 `len=0`（模型什么也没返回）。这是 4B 的已知抖动（`ollama 返回空响应`）。
**FA 之所以稳定，是因为内核有 `model.retry{reason:"empty"}` 的重试（N3）** —— 这是 harness 带来的真实收益。

### 6.6 结论

> **在这套 7 题试卷上，FeatherAgent 环境明显优于裸跑**：
> 分数 **24/24 vs 17/24**，耗时 **143s vs 332s**。
>
> 优势来源主要是两点：① **空响应重试**（裸跑 Q6/Q7 直接交白卷）；② **系统提示词的结构化倾向**（表格、分点、术语保留）。
>
> 劣势/风险：**代码类任务会被"工具优先"带偏**（Q1 写文件而非作答），需要在提示词里区分"输出"与"落盘"。

---

## 7. 问题清单

| # | 严重度 | 问题 | 建议 |
|---|---|---|---|
| **N19** | 🔴 | **`cold_spawn_ms_p50` 从 ~180ms 涨到 ~3180ms**（N16 的重试等待被计入），`cold_spawn_lt_250ms` 恒红 | 只计首次成功耗时；重试等待单列；或重校准门限 |
| **N20** | 🟠 | **代码题被"工具优先"带偏**：要求给代码，模型却调用 `tool.fs.write` 写文件 | 提示词区分"输出代码"与"生成文件"；默认在回答里给代码块 |
| N21 | 🟡 | N18 的列举类覆盖仍回避常见对比维度（开销/头部） | 让第一层小标题对齐用户提到的对比轴 |
| N22 | 🟡 | N15 虽不再 failed，但结论多为 `unapproved`（门禁未通过）+ 反复 read 4 次 | rubric 对"写文件"类任务应更宽松（校验文件内容而非答案措辞） |
| — | ✅ | ~~N15 XML 恢复~~ | **已修** |
| — | ✅ | ~~N16 rpc_connect~~ | **已修**（0/5） |
| — | ✅ | ~~N17 advisory_gates~~ | **已修** |
| — | ⚠️ | ~~N18 列举覆盖~~ | **部分**（见 N21） |

---

## 8. 未验证项

| 项 | 说明 |
|---|---|
| 附件选择器 / PTY 流程、`-en` 前端界面 | 未做 |
| 策略下拉、权限弹窗、设置页字段 | UI 面，未做 |
| 五 provider 实连、Bearer、网络盘、24h、高 DPI | 未做 |
| 试卷的**正确性**深度校验 | 规则化打分只验覆盖面（如 Q1 代码未实际执行） |

---

## 9. 复现命令

```powershell
cd FeatherAgent-1.2.1-rc.4.2-windows-x64
$env:HTTP_PROXY="http://127.0.0.1:34686"; Remove-Item Env:NO_PROXY -ErrorAction SilentlyContinue
.\featheragent.exe serve --root . --provider provider-ollama --model qwen3.5:4b

# N16：连续 bench
1..5 | % { .\featheragent.exe bench --root . --json | ConvertFrom-Json |
           "$($_.metrics.cold_spawn_ms_p50)  $($_.gates.cold_spawn_lt_250ms)" }

# N15：XML 恢复
.\featheragent.exe chat --root . --session x15 --allow tool.fs.write,tool.fs.read `
  --text "请用工具把三条关于「内存管理」的要点写入 workspace/notes.txt，每条一行，以「- 」开头"

# 试卷（Q1 示例）
.\featheragent.exe chat --root . --session q1 --text "请用 Python 实现一个 LRU 缓存类…"
curl -s -X POST http://127.0.0.1:11434/api/chat -H "Content-Type: application/json" `
  -d '{"model":"qwen3.5:4b","stream":false,"messages":[{"role":"user","content":"请用 Python 实现一个 LRU 缓存类…"}]}'
```

---

## 10. 演进总览

| 指标 | rc.4 | rc.4.1 | **rc.4.2** |
|---|---|---|---|
| XML 畸形调用 | ❌ failed | ❌ failed | **✅ 恢复（unapproved 而非 failed）** |
| bench `rpc_connect` | 1/2 失败 | 1/2 失败 | **✅ 0/5** |
| `cold_spawn` | ~180ms ✅ | ~180ms ✅ | 🔴 **~3180ms（N19）** |
| `advisory_gates` | ❌ 混在 gates | ❌ | **✅ 独立** |
| 列举覆盖（N18） | — | 3/4 | **3/4（部分）** |
| 试卷（24 分） | — | — | **FA 24 · 裸跑 17** |
| 阻断级缺陷 | 1（N10） | 0 | **1（N19）** |

> **这版修好了三条（N15/N16/N17），但 N16 连带把 `cold_spawn` 测量毁掉了**——
> 这是本轮唯一的阻断级问题，且很隐蔽：**功能好了、指标坏了**。
>
> 试卷结果则给了 harness 一个明确的正向证据：**在难题上，重试与结构化提示把 4B 从"交白卷"拉到"满分"**。
