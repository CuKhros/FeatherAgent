# FeatherAgent v1.2.1-rc.3.5 · Windows 真机复测报告

> 测试机：Windows x64 · Ollama 0.32.9 · qwen3.5:4b(exec) / qwen2.5-coder:7b(guide, judge)
> 复测时间：2026-11-25
> 被测包：`FeatherAgent-1.2.1-rc.3.5-windows-x64`
> 对照：`Windows真机复测报告-v1.2.1-rc.3.4.md`（M1 / M3）

---

## 1. 结论（对照清单三项）

| 清单项 | 结果 | 一句话 |
|---|---|---|
| **1. 复跑 a2：占位符判据应被丢弃并继续，不再 degraded** | ✅ **完全达成** | 2/2 **approved**（rounds=1，judge 未参与），rubric 四项全是真判据 |
| **2. 复跑 a1：主观项不得进 judge，整轮不得因它 unapproved** | ⚠️ **部分达成** | 主观项**确实被排除**了，但整轮**仍 unapproved**——换成了 `must_include` 字面匹配失败 |
| **3. S3 t6 连跑多次，每次一次 `tool.fs.read`** | ⚠️ **4/5 达成** | 5 次里 4 次恰好 1 次调用；但 2 次整轮 failed（模型输出畸形 XML / 空响应） |

**另发现 3 个新问题（N1 最关键）：`must_include` 对概念性短语太脆；`boot_ms_p50/p90` 与样本不同源。**

---

## 2. 清单项 1：a2 完全修复 ✅

### 2.1 结果：2/2 approved，不再 degraded

```
warn:     没有描述型判据：judge 不参与本轮，全部由确定性判据决定
verdict:  pass=True  failed=[]  votes=0
finished: status="approved"  rounds=1
```

**三次尝试（rc.3.4）→ 两次一轮通过（rc.3.5）。** 而且这次 **judge 完全没被调用**（`votes=0`），
纯 hardcheck 判定，成本 = guide 1 次 + exec 1 次。

### 2.2 判据不再出现占位符

b21 的 guide 产物原样（`data/review-artifacts/b21/*-guide-a1.json`）：

```json
{"id":"c1","criterion":"必须包含 TCP 三次握手的简要说明","must_include":["TCP 三次握手"]}
{"id":"c2","criterion":"必须提到 SYN 和 ACK 的作用","must_include":["SYN","ACK"]}
{"id":"c3","criterion":"至少包含三个要点","min_points":3}
{"id":"c4","criterion":"不能使用缩写或非正式语言","must_not_include":["简","快"]}
```

- **没有任何"禁止使用示例中的占位词"这类自指判据** —— `__FILL_*__` 改造 + 禁谈占位符的元指令生效
- **`must_include` 数组被接受并全部要求**（M1b）：`["SYN","ACK"]` 两个词都必须出现
- 解析后 4 项全部保留，无丢弃

> 注：本轮占位符判据**根本没生成**，所以"丢弃"分支未被触发。建议保留 rc.3.4 的 a2 产物作为该分支的回归用例
> （现在 `data/review-artifacts/a2/` 已被清空，如需回归可手工构造 raw 输入）。

---

## 3. 清单项 2：a1 部分达成 ⚠️

### 3.1 达成的一半：主观项确实不进 judge

| 运行 | warn |
|---|---|
| a11 | `没有描述型判据：judge 不参与本轮，全部由确定性判据决定` |
| a12 | `1 条描述型判据（c4）：由 judge 按 criterion 评估` |

**a11 里 c4 被彻底排除**（advisory），judge 一次没调。
a12 里 c4 被判为"可执行"，进了 judge——说明**主观项识别是概率性的**（取决于 guide 当次怎么写 c4）。

### 3.2 未达成的一半：整轮仍 unapproved，但原因换了

两次运行都是 `failed=['c1']`、`votes=0`（**纯 hardcheck**，judge 无关）。看 a11 的判据：

```json
// attempt 1（被拒）
c1 must_include ["操作系统是", "管理计算机硬件和软件资源"]   ← 整句 → 拒绝
// attempt 2（接受）
c1 must_include ["软件", "管理计算机硬件和资源"]
```

而 exec 的实际答案写的是：

> 操作系统是管理计算机硬件**与软件**资源…

**`"管理计算机硬件和资源"` 与 `"管理计算机硬件与软件资源"` 不是连续子串** —— 多了"与软件"三个字就匹配不上。

→ **c1 三轮全 fail → unapproved**。**与主观项无关，是字面匹配太脆（N1）。**

### 3.3 一个正面细节：整句判据会被拒并重试

```
warn: 第 1 次判据不合格：rpc -32000: 验收项 1 的 must_include 是整句：判据只能是关键词/短语
warn: 第 2 次判据不合格：…（a12 连拒两次后第 3 次通过）
```

**"整句 → 关键词"的校验 + 重试闭环工作正常**（a12 重试 2 次后成功）。

### 3.4 🟡 N4：fixture 词仍在渗透

a11 attempt 1 的 c4：

```json
{"id":"c4","criterion":"避免照搬答案",
 "check":"没有直接复制示例中的句子或段落",
 "must_not_include":["REVIEW-FINAL","REVIEW-START","<...>"]}
```

模型仍在写"避免照搬示例"并列出 fixture 词。这次被"整句 must_include"的规则**间接**挡下了（同一份 rubric 的 c1 不合格），
但它本身**没被丢弃逻辑命中**——因为 `REVIEW-FINAL` 是 `must_not_include`，且不在用户输入里，
changelog 说这种情况会拒绝……hmm，实际是**整份 rubric 因 c1 被拒**，c4 这条没被单独判死。

**建议**：`must_not_include` 里出现 fixture 词（`REVIEW-*` / `<...>` / `__FILL_*__`）应**直接丢弃该项**（与 M1 的丢弃逻辑对齐），
而不是依赖"其他项不合格"来间接拦截。

---

## 4. 清单项 3：S3 t6 连跑 5 次 ⚠️

### 4.1 结果

| # | tool.call 次数 | 工具名 | status | 答案 |
|---|---|---|---|---|
| 1 | 1 | `tool.fs.read` | ❌ **failed** | （空） |
| 2 | 1 | `tool.fs.read` | ✅ ok | 该文件不存在（`workspace/no-such-file-xyz.txt` 找不到） |
| 3 | **0** | — | ❌ **failed** | （空） |
| 4 | 1 | `tool.fs.read` | ✅ ok | 该文件…不存在。工具尝试读取时明确报错… |
| 5 | 1 | `tool.fs.read` | ✅ ok | workspace/no-such-file-xyz.txt 未找到 |

**"每次一次 `tool.fs.read`" 成立 4/5（第 3 次是 0 次）。**

### 4.2 两次失败的根因：都在模型侧

**第 1 次**：

```
tool.call tool.fs.read → tool.error（文件不存在，正确）
→ 模型下一步输出畸形 XML：
chat.finished {"error":"XML syntax error on line 3: element <function> closed by </parameter>"}
```

**第 3 次**：

```
chat.finished {"error":"ollama 返回空响应（content 与 tool_calls 均为空）…"}
```

**关键判断：内核侧行为正确。**
- 工具调用发生了（第 1 次）
- 文件不存在的业务错误被正确回注（`rpc -32000: …\workspace\default\no-such-file-xyz.txt …`）
- 失败发生在**模型生成最终答复**这一步：一次输出畸形 XML、一次输出空

### 4.3 建议

| 优先 | 措施 |
|---|---|
| **1** | **工具调用格式解析失败应回注给模型重试**（提示"你的 function call 格式错误，请用标准 JSON"），而不是把整轮判 failed。现在 `model.retry` 只重试 1 次就放弃 |
| **2** | **空响应重试提高到 2 次**（4B 偶发，本会话已多次观测到） |
| **3** | 若连续两次格式失败，可回退为"纯文本作答 + 明确告知工具不可用"，至少给用户一个可用答复 |

---

## 5. 🔴 N1（新，关键）：`must_include` 对概念性短语太脆

### 5.1 现象

| guide 写的判据 | exec 实际答案 | 结果 |
|---|---|---|
| `must_include: ["管理计算机硬件和资源"]` | "管理计算机硬件**与软件**资源" | ❌ 差 3 个字 → 不匹配 |
| `must_include: ["Windows","Linux"]` | 答案未提 Windows/Linux | ❌ 必然失败 |

### 5.2 这是字面匹配的结构性矛盾

| 短语长度 | 问题 |
|---|---|
| 太短（`"软件"`） | 几乎必然出现 → 判据无意义 |
| 适中（`"SYN"`/`"ACK"`/`"Windows"`） | ✅ 合理：**专有名词/术语** |
| 稍长（`"管理计算机硬件和资源"`） | ❌ 自然语言措辞一变就失配 |
| 整句 | ❌ 已被校验拒绝 |

**核心结论：`must_include` 只适合"专有名词 / 术语 / 固定写法"，不适合"概念性描述"。**

而 guide（7B）没有这个分寸感——它会把"概念要求"也写成 `must_include`。

### 5.3 建议

| 优先 | 措施 |
|---|---|
| **1** | **限长 2~6 字**，且**禁止含"和/与/的/是"等连接词**（现在的"整句"检测只拦句号结尾，`"管理计算机硬件和资源"` 就溜过去了） |
| **2** | 或改成 **token 级匹配**：短语拆词后要求全部出现（不要求连续）→ `管理/计算机硬件/资源` 三词都在即可 |
| **3** | 或**白名单化**：`must_include` 只允许"不在用户输入中出现过的新术语"以外的场景，概念性要求一律走 `description` 交 judge |
| **4** | guide prompt 里明确举例：`must_include` 用于 `SYN`、`Windows`、`进程调度` 这类**术语**，不用于描述性句子 |

> 这条不修，**a1 这类任务会稳定 unapproved**（因为 guide 总是倾向于写长短语），
> 且失败原因看起来像"答案不合格"，极具误导性。

---

## 6. 🟠 N2（新）：`boot_ms_p50` / `boot_ms_p90` 与 `boot_samples_ms` 不同源

### 6.1 三次实测

| # | `boot_samples_ms` | 真实中位 | 报告 `boot_ms_p50` | 报告 `boot_ms_p90` |
|---|---|---|---|---|
| 1 | 36.9, 35.6, 39.1, 56.3, 36.5 | **36.9** | **39.09** | **36.52** |
| 2 | 44.2, 65.3, 36.7, 54.2, 49.4 | **49.4** | **36.66** | 49.40 |
| 3 | 33.0, 33.8, 46.4, 37.2, 47.7 | **37.2** | **46.38** | 47.73 |

**三个数字彼此对不上**：

- 第 1 次：P90（36.52）**小于** P50（39.09）—— 数学上不可能
- 第 2 次：P50（36.66）≈ 样本最小值；P90（49.40）≈ 样本中位数
- 第 3 次：P50（46.38）≈ 样本第 3 大

**结论：P50/P90 不是从 `boot_samples_ms` 这一组数算出来的。**

### 6.2 影响

- 门禁 **用 P50 判定**，而这个 P50 与它自己打印的样本对不上 → **读数不可核查**
- 如果 P50 恰好取到了偏小的那个样本，会**假绿**；反之**假红**

### 6.3 建议

1. **同一个数组算 P50/P90**，并在输出里保证 `P50 ≤ P90`（加一条自检，违反就报 `metric_inconsistent`）；
2. 或干脆只输出 `boot_samples_ms`，让门禁和报表各自算——**少一层加工就少一处错**。

> 这条恰好说明"暴露样本"这个设计是对的：**没有 `boot_samples_ms`，这个 bug 永远不会被发现。**

---

## 7. S3 完整回归与 bench

### 7.1 S3（满分）

```
应触发工具的任务数        : 5
  实际触发                : 5  (5/5)
  工具名命中              : 5  (5/5)
  工具执行成功（严格口径）  : 4  (4/5)
  预期业务错误且答复正确     : 1  (预期负样本 1 个)
  真失败                    : 0  (0/5)
  最终答复正确            : 4  (4/5)
不应调用工具的任务        : 1，正确未调用 1

写入校验 workspace/w1.txt : 存在 -> 'HELLO_W_99'      ✅
```

t6 在这一轮是 **单次 `tool.fs.read` + 预期失败 + 答复正确 ✅**

### 7.2 bench

```
gates 7/7 全部通过
warm P50=45.5  P90=43.27（← 见 §6，数值异常）
env: mem_load_pct=85%, reliability="low"
```

---

## 8. 问题清单

| # | 严重度 | 问题 | 建议 |
|---|---|---|---|
| **N1** | 🔴 | **`must_include` 对概念性短语太脆**：`"管理计算机硬件和资源"` vs 答案 `"…硬件与软件资源"` → 差 3 字即失配 → a1 稳定 unapproved | 限长 2~6 字 + 禁连接词；或 token 级匹配；或用术语白名单 |
| **N2** | 🟠 | **`boot_ms_p50` / `p90` 与 `boot_samples_ms` 不同源**，出现过 P90 < P50 | 同源计算 + `P50 ≤ P90` 自检；或只输出样本 |
| **N3** | 🟡 | 工具调用畸形 XML / 空响应 → 整轮 failed，仅重试 1 次 | 格式错误回注重试；空响应重试 2 次；两次失败后回退纯文本 |
| **N4** | 🟡 | `must_not_include` 里的 fixture 词（`REVIEW-*`/`<...>`）未被单独丢弃，靠"其他项不合格"间接拦截 | 与 M1 的丢弃逻辑对齐 |
| — | ✅ | ~~M1 占位符判据导致 degraded~~ | **已修**（a2 2/2 approved） |
| — | ✅ | ~~M3 主观项进 judge~~ | **已修**（advisory 排除；a11 judge 未参与） |
| — | ✅ | ~~整句 `must_include`~~ | **已修**（拒绝 + 重试，a12 重试两次后通过） |
| — | ✅ | ~~`must_include` 数组~~ | **已修**（`["SYN","ACK"]` 全部要求） |

---

## 9. 未验证项

| 项 | 说明 |
|---|---|
| 策略下拉 / ConPTY Enter 保存 / 权限弹窗 / 设置页 14 字段 | UI 与 PTY 面，未做 |
| 占位符判据"丢弃并继续"分支 | 本轮占位符判据未生成，丢弃逻辑未被触发（建议用 rc.3.4 的 a2 raw 做回归用例） |
| K2 漏项补投 | judge 未漏项，未触发 |
| 五 provider 实连、Bearer、网络盘、24h、高 DPI | 未做 |

---

## 10. 复现命令

```powershell
cd FeatherAgent-1.2.1-rc.3.5-windows-x64
$env:HTTP_PROXY="http://127.0.0.1:34686"; Remove-Item Env:NO_PROXY -ErrorAction SilentlyContinue
.\featheragent.exe serve --root . --provider provider-ollama --model qwen3.5:4b
.\featheragent-webui.exe --root .
# config.set: guide/judge.model=qwen2.5-coder:7b, review_gate.enabled=true

# 清单1：a2 应 2/2 approved 且 votes=0
.\featheragent.exe chat --root . --session b2 --text "用三句话说明 TCP 三次握手，必须提到 SYN 和 ACK"

# 清单2：a1 主观项不进 judge（看 warn），但 likely unapproved（N1）
.\featheragent.exe chat --root . --session a1 --text "用三句话说明什么是操作系统"

# 清单3：t6 连跑 5 次，统计 tool.call 次数
1..5 | % { .\featheragent.exe chat --root . --session t$_ --text "请读取 workspace/no-such-file-xyz.txt 并告诉我内容。" }

# N2：核对 P50/P90 与 samples
.\featheragent.exe bench --root . --json | ConvertFrom-Json |
  Select-Object -ExpandProperty metrics | Select boot_samples_ms, boot_ms_p50, boot_ms_p90
```

---

## 11. 演进总览

| 指标 | rc.3.3 | rc.3.4 | **rc.3.5** |
|---|---|---|---|
| a2（TCP） | degraded | degraded | **✅ 2/2 approved** |
| a1（操作系统） | — | unapproved | **unapproved（换因：N1）** |
| 主观项进 judge | 是 | 是 | **否（advisory）** |
| 占位符判据 | 生成即降级 | 生成即降级 | **✅ 不再生成** |
| 整句 must_include | 放行 | 放行 | **✅ 拒绝 + 重试** |
| S3 完整 | 4/5 触发 | 4/5 触发 | **✅ 5/5 · 真失败 0** |
| S3 t6 单次调用 | — | 1/1 | **4/5** |
| bench | 7/7 | 7/7 | **7/7（P90 数据异常）** |
| 阻断级缺陷 | 1（L1） | 0 | **1（N1）** |

> **这版把 M1 彻底解决了**：a2 从"三次重试全挂"变成"两次一轮通过，且 judge 不用出场"——
> 这是 review-gate 目前最干净的一次通过（guide 1 + exec 1，共 2 次模型调用）。
>
> 但 **N1 是新的结构性障碍**：`must_include` 被 guide 用来表达"概念要求"，而字面匹配对措辞极其敏感。
> 这不是"再给模型一点提示"能解决的，需要在**判据类型**上做区分：术语用字面，概念用 description。
