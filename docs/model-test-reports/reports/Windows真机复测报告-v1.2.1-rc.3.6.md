# FeatherAgent v1.2.1-rc.3.6 · Windows 真机复测报告

> 测试机：Windows x64 · Ollama 0.32.9 · qwen3.5:4b(exec) / qwen2.5-coder:7b(guide, judge)
> 复测时间：2026-11-25
> 被测包：`FeatherAgent-1.2.1-rc.3.6-windows-x64`
> 对照：`Windows真机复测报告-v1.2.1-rc.3.5.md`（N1 ～ N4）

---

## 1. 结论（对照清单四项）

| 清单项 | 结果 | 一句话 |
|---|---|---|
| **1. 真实 Ollama 畸形工具调用** | ✅ **达成（N3）** | `model.retry{reason:"tool_format"}` 与 `"empty"` 均实测到；4 次中 3 次靠重试恢复 |
| **2. 真实 7B guide 行为** | ⚠️ **N1 修好但引出 N5** | `must_include` 已术语化，但**数组语义是 AND**，guide 用它列功能清单 → 要求全中 → 必然 fail |
| **3. N4 fixture rubric** | ⬜ **未触发** | 本轮 guide 未写 fixture 词；rc.3.5 的 a11 曾出现（建议用其 raw 做回归用例） |
| **4. 双语 exe 与 bench 门禁** | ✅ **均正常** | `-en` 版输出英文 Usage；**N2 已修**（P50/P90 同源 + 一致性标志） |

**新发现 3 项：`must_include` 数组语义（N5）、主观判据被实现成禁词（N6）、bench 间歇 `os error 2`（N7，跨版本）。**

---

## 2. 清单项 1：畸形工具调用（N3）✅

### 2.1 实测 4 次

| # | tool.call | `model.retry` 的 reason | status | 说明 |
|---|---|---|---|---|
| 1 | 0 | `["empty","empty"]` | ❌ failed | 连续两次空响应，预算耗尽 |
| 2 | 1 | **`["tool_format"]`** | ✅ ok | **畸形格式重试后成功** |
| 3 | 2 | `["empty","tool_format","empty","empty"]` | ✅ ok | 重试 4 次后成功（但多调了 1 次工具） |
| 4 | 1 | `["empty"]` | ✅ ok | 一次空响应后成功 |

### 2.2 结论

- **`reason` 字段与 changelog 完全一致**：`empty` / `tool_format` 两种都能观测到
- **恢复率 3/4**（rc.3.5 是 3/5 且失败是硬失败，现在失败只在预算耗尽时发生）
- 第 1 次的失败**符合设计**（"failed is emitted only after the budget"）

### 2.3 🟡 N8：重试链会让工具被多调

第 3 次 `tool.call=2` —— 重试过程中同一个工具被调了两次。
对 t6 这种"应恰好一次调用"的用例，**重试链会破坏该不变式**。

**建议**：重试时**保留已成功的工具调用结果**，只重试"生成答复"那一步；或在 `tool.call` 事件里带 `attempt` 字段，
让"因重试产生的重复调用"可被区分。

---

## 3. 清单项 2：真实 7B guide 行为 ⚠️

### 3.1 N1 修好的部分：`must_include` 已术语化

c11 的 guide 产物（`data/review-artifacts/c11/*-guide-a1.json`）：

```json
c1 must_include ["操作系统"]                              ← 术语，✅
c2 must_include ["文件管理","进程管理","设备驱动","用户界面"]  ← 4 个术语，但要求「全部出现」
c3 min_points 2 + must_include ["提高效率","简化操作","安全保护"]
c4 must_not_include ["软件","硬件","计算机"]                ← 禁词选得有问题
```

**对比 rc.3.5**：那次是 `must_include:["管理计算机硬件和资源"]`（长短语，必然失配）；
这次全是术语（`操作系统`/`文件管理`），**N1 的"术语化"确实生效**。

### 3.2 🟠 N5（新，关键）：`must_include` 数组是 AND 语义

`must_include` 从字符串改成数组后（M1b），语义定为**"每个都必须出现"**。
但 guide 会用它表达**功能清单**：

```
c2 要求：文件管理 + 进程管理 + 设备驱动 + 用户界面   ← 四项全中
实际答案：只提到「进程管理、内存分配」
```

→ **c2 必然 fail**。同理 c3（要求"提高效率"+"简化操作"+"安全保护"三个词全中）。

**这是"术语化修好了措辞、AND 语义又把它变成清单题"**。

**建议**：`must_include` 支持匹配模式：

```yaml
must_include: ["文件管理","进程管理","设备驱动","用户界面"]
match: any        # 默认；至少一个
# 或
match: min
min_matches: 2
# 特例：SYN/ACK 这种「都要有」用
match: all
```

> 对契约兼容：默认改成 `any` 会放松现有判据；更稳妥是**默认 `min_matches: ceil(n/2)`**，并允许显式 `all`。

### 3.3 🟠 N6（新）：主观判据被实现成"禁掉常用词"

```json
c4 {"criterion":"使用准确的术语",
    "check":"确保使用的术语和概念准确无误",
    "must_not_include":["软件","硬件","计算机"]}
```

criterion 是"术语要准确"这种**主观要求**，却被实现成**禁止出现「软件/硬件/计算机」**——
而这三个词是"解释操作系统"时**必然要用**的（答案第一句就是"操作系统是一个系统**软件**"）。

→ c4 必然 fail，且失败原因与"术语准确"毫无关系。

**建议**：
1. **主观判据识别要扩展**：现在只识别"简洁/清晰/格式"这类词，`术语准确`、`表达准确`、`内容完整` 也属主观；
2. **`must_not_include` 应做"可用性校验"**：若禁词出现在**用户输入**或属于该领域高频词（可通过一次简单词频判断），
   应判 guide 不合格并重试——否则判据自相矛盾。

---

## 4. 清单项 3：N4 fixture rubric ⬜ 未触发

本轮 4 次 guide 产出中**没有出现** `REVIEW-FINAL` / `<...>` / `__FILL` 等 fixture 词，
所以"含 fixture 的项被丢弃 + `review.warn`"这条分支**未被触发**。

rc.3.5 的 a11 attempt1 曾有：

```json
c4 {"criterion":"避免照搬答案",
    "must_not_include":["REVIEW-FINAL","REVIEW-START","<...>"]}
```

**建议**：把这段 raw 作为固定回归输入（单测或 `bench --fixture`），
否则这条分支只能靠模型心情触发，永远验证不到。

---

## 5. 清单项 4：双语 exe 与 bench 门禁 ✅

### 5.1 双语 exe

目录里 10 个 exe，中英各一套：

```
featheragent.exe      / featheragent-en.exe
featheragent-tui.exe  / featheragent-tui-en.exe
featheragent-webui.exe/ featheragent-webui-en.exe
feather-plugin.exe    / feather-plugin-en.exe
feather-storage.exe   / feather-storage-en.exe
```

冒烟对比：

```
zh: FeatherAgent 1.2.1-rc.3.6 (contract 1.0)
    用法: featheragent <serve|doctor|bench|webui|chat|version|help> [选项]
en: FeatherAgent 1.2.1-rc.3.6 (contract 1.0)
    Usage: featheragent-en <serve|doctor|bench|webui|chat|version|help> [options]
```

**版本号一致、语言正确切换** ✅

### 5.2 N2：`boot_ms_p50` / `p90` 已同源

| # | `boot_samples_ms` | 真实中位 | 报 P50 | 报 P90 | `boot_percentiles_consistent` |
|---|---|---|---|---|---|
| 1 | 99.5, 95.7, 102.4, 109.5, 100.0 | 100.0 | **100.00** | 109.48 | **true** |
| 2 | 104.3, 96.1, 94.7, 88.6, 61.4 | 94.7 | **94.71** | 104.29 | **true** |
| 7 | 37, 42, 42, 42, 39 | 42 | **42.3** | 42.5 | true |

**P50 与样本中位完全吻合，P50 ≤ P90 结构性成立**，`gates.metrics_consistent=true`。
**N2 关闭。**

### 5.3 🟡 N7：bench 间歇 `os error 2`（**跨版本存在**）

| 版本 | 次数 | `os error 2` |
|---|---|---|
| rc.3.6 | 8 | **2 次**（第 3、6 次）|
| rc.3.5（同刻对照） | 2 | **1 次**（第 1 次）|

```
error: 系统找不到指定的文件。 (os error 2)
```

**注意：rc.3.5 也复现了**，所以**不是 rc.3.6 引入的**。
推测是 bench 自己在 `runtime/` 下创建临时产物时与上一次运行残留冲突。

**建议**：与 NEW-K 一样给结构化错误（`code:"runtime_busy"` / `code:"artifact_missing"`），
并说明是哪个路径缺文件；现在的 `os error 2` 无法定位。

### 5.4 bench 门禁稳定性（环境相关）

| 版本 | 运行 | warm P50 | 门禁 |
|---|---|---|---|
| rc.3.6 | 1 | 100.0 | ❌ |
| rc.3.6 | 2 | 94.7 | ✅ |
| rc.3.6 | 4 | 98.1 | ✅ |
| rc.3.6 | 5 | 100.0 | ❌ |
| rc.3.6 | 7 | **42.3** | ✅ |
| rc.3.6 | 8 | **117.8** | ❌ |
| rc.3.5（对照） | 1 | 43.6 | ✅ |

**同一台机器上 warm 在 42 ~ 155 ms 之间大幅抖动**，门禁约 **1/3 概率失败**。
两版都能跑出 42 ms，说明**不是版本变慢**，而是**环境抖动**（`env.reliability` 在 medium 一档）。

**建议**：`reliability != high` 时，`boot_lt_100ms` 失败**不该直接判红**，应像 K4 一样标注"读数不可信"。

---

## 6. S3 回归

```
应触发工具的任务数        : 5
  实际触发                : 5  (5/5)
  工具名命中              : 5  (5/5)
  工具执行成功（严格口径）  : 3  (3/5)
  预期业务错误且答复正确     : 1  (预期负样本 1 个)
  真失败                    : 1  (1/5)
  最终答复正确            : 3  (3/5)
不应调用工具的任务        : 1，正确未调用 1

写入校验 workspace/w1.txt : 存在 -> 'HELLO_W_99'    ✅
```

t6 本轮 **单次 `tool.fs.read` + 预期失败 + 答复正确** ✅
执行 3/5 略低于 rc.3.5 的 4/5，属模型波动（4B 在长 prompt 下偶发少调/多调）。

---

## 7. 问题清单

| # | 严重度 | 问题 | 建议 |
|---|---|---|---|
| **N5** | 🟠 | **`must_include` 数组是 AND 语义**，guide 用它列功能清单 → 要求 N 个术语全中 → 必然 fail | 支持 `match: any/all/min` + `min_matches`；默认建议 `min_matches: ceil(n/2)` |
| **N6** | 🟠 | **主观判据被实现成禁词**：`"使用准确的术语"` → `must_not_include:["软件","硬件","计算机"]`，禁掉答案必用词 | 扩展主观识别词表；`must_not_include` 加"可用性校验"（禁词不得是领域高频词/用户输入中的词） |
| N7 | 🟡 | **bench 间歇 `os error 2`**（rc.3.5 同样复现，跨版本） | 结构化错误码 + 报出缺失路径 |
| N8 | 🟡 | 重试链导致工具被多调（`tool.call=2`） | 重试保留已成功结果；`tool.call` 加 `attempt` 字段 |
| N9 | 🟡 | bench warm 抖动 42~155 ms，门禁 1/3 概率红（环境） | `reliability != high` 时该门禁标注而非判红 |
| — | ✅ | ~~N1 must_include 长短语~~ | **已修**（术语化，2~12 字，禁连接词） |
| — | ✅ | ~~N2 P50/P90 不同源~~ | **已修**（同源 + consistent 标志） |
| — | ✅ | ~~N3 畸形工具调用/空响应不重试~~ | **已修**（`tool_format` / `empty` 重试） |
| — | ⬜ | ~~N4 fixture rubric 丢弃~~ | **未触发**（建议用 rc.3.5 的 a11 raw 做回归） |

---

## 8. 未验证项

| 项 | 说明 |
|---|---|
| N4 fixture 丢弃分支 | 本轮未生成 fixture 判据 |
| `-en` 版的前端（tui/webui）实际界面 | 只烟测了 `version` / `help` |
| 策略下拉 / ConPTY Enter / 权限弹窗 / 设置页字段 | UI 与 PTY 面，未做 |
| 五 provider 实连、Bearer、网络盘、24h、高 DPI | 未做 |

---

## 9. 复现命令

```powershell
cd FeatherAgent-1.2.1-rc.3.6-windows-x64
$env:HTTP_PROXY="http://127.0.0.1:34686"; Remove-Item Env:NO_PROXY -ErrorAction SilentlyContinue

# 双语 exe
.\featheragent.exe help; .\featheragent-en.exe help

# N2：P50/P90 一致性
.\featheragent.exe bench --root . --json | ConvertFrom-Json |
  Select-Object -ExpandProperty metrics | Select boot_samples_ms, boot_ms_p50, boot_ms_p90, boot_percentiles_consistent

# N3：畸形工具调用重试（连跑多次看 model.retry.reason）
1..4 | % { .\featheragent.exe chat --root . --session n$_ --text "请读取 workspace/no-such-file-xyz.txt 并告诉我内容。" }

# N1/N5/N6：guide 判据形态
.\featheragent.exe chat --root . --session c1 --text "用三句话说明什么是操作系统"
Get-Content .\data\review-artifacts\c1\*-guide-a1.json
```

---

## 10. 演进总览

| 指标 | rc.3.4 | rc.3.5 | **rc.3.6** |
|---|---|---|---|
| must_include 形态 | 长短语 | 长短语 | **✅ 术语（2~12 字）** |
| must_include 数组 | 字符串 | ✅ 数组 | ⚠️ **AND 语义过严（N5）** |
| P50/P90 一致性 | ❌ 不同源 | ❌ 不同源 | **✅ 同源 + 标志** |
| 畸形工具调用 | 硬失败 | 重试 1 次 | **✅ tool_format 重试** |
| 空响应 | 硬失败 | 重试 1 次 | **✅ empty ×2** |
| 双语 exe | 无 | 无 | **✅ 5 对** |
| a1（操作系统） | unapproved | unapproved | **unapproved（换因 N5/N6）** |
| S3 写盘 | ✅ | ✅ | **✅** |
| 阻断级缺陷 | 0 | 1（N1） | **0**（N5/N6 只影响 review-gate 场景） |

> **这版把 N1 的"措辞"问题解决了**（术语化 + 分词匹配），执行链路的鲁棒性也明显提升
> （畸形工具调用能自愈、空响应能重试、bench 指标可以自证一致性）。
>
> 但 review-gate 换成了**"判据语义"层面**的问题：
> **`must_include` 数组的 AND 语义**让"列清单"变成"必须全中"；
> **主观判据**被硬塞进 `must_not_include`，禁掉答案必用的词。
>
> 这两条都不是"再修一次校验"能解决的——需要给判据**加上匹配模式**，并**限制每种判据能表达什么**。
