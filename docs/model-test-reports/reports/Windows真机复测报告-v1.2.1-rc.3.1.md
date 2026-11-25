# FeatherAgent v1.2.1-rc.3.1 · Windows 真机复测报告

> 测试机：Windows x64（同前）· Ollama 0.32.9
> 复测时间：2026-11-24（20:00 后）
> 被测包：`FeatherAgent-1.2.1-rc.3.1-windows-x64`
> 对照：`Windows真机复测报告-v1.2.1-rc.3.md`（I1 ～ I4）

---

## 1. 结论

**I1/I2 修复成功：`review-gate` 从"4/4 全降级"变成"1/3 批准、2/3 合理拒绝"——质量门第一次真正在起作用。**
**"宽容降级"（我上轮建议 #3）落地为 `review.warn` + judge-only，效果很好。**

**但暴露两个新问题：`min_points` 判定过粗导致误杀；exec 把 rubric 编号写进答案。**

> ⚠️ 另需说明：本轮 bench 数据**不可用于版本对比**——机器进入降级状态，rc.3 与 rc.3.1 同等劣化（见 §6）。

| 项 | rc.3 | rc.3.1 | 判定 |
|---|---|---|---|
| **I1** 真实模型全降级 | 🔴 4/4 degraded | ✅ **1/3 approved · 2/3 unapproved** | 修好 |
| **I2** guide 拿到 schema + 错误回注 | ❌ | ✅ **guide 一次成功**（`attempts:1`） | 修好 |
| 宽容降级（建议 #3） | ❌ 直接 degraded | ✅ **`review.warn` + judge-only** | 修好 |
| 答案质量 | 抄指导者的句子 | ✅ **252 字结构化答案** | 显著改善 |
| **J1** `min_points` 误杀 | — | 🟠 **2/3 轮被误判** | 新问题 |
| **J2** exec 把 `c1/c2/c3` 写进答案 | — | 🟠 **出现** | 新问题 |
| S3 | 5/5 触发 | ✅ **5/5 触发 · 答复 4/5 · 写盘 ✅** | 保持 |
| bench | 7/7（早先） | ⚠️ **环境漂移，不可比** | 见 §6 |

---

## 2. I1/I2 验证：review-gate 真的开始工作了

配置：`guide=qwen2.5-coder:7b`、`judge=qwen2.5-coder:7b`、`exec=qwen3.5:4b`，连跑 3 次。

### 第 1 次：**approved（rounds=2）**

```
review.warn: {attempts:1, model:"qwen2.5-coder:7b",
              text:"2 条判据只有描述、没有硬检查（c1, c2）：交给 judge 评估"}

review.verdict items:
  c1 {criterion:"检查是否提供了操作系统的基本定义",
      check:"是否包含操作系统的核心功能和作用",
      evidence:"操作系统（Operating System）", pass:true}
  c2 {criterion:"检查是否说明了操作系统的作用",
      check:"是否解释了操作系统如何帮助用户管理和使用计算机资源",
      evidence:"OS 作为系统内核，负责调度 CPU、内存和存储等硬…", pass:true}

review.finished {rounds:2, status:"approved"}
chat.finished   {"status":"ok","review":{"approved":true,"degraded":false,"rounds":2,"status":"approved"}}
```

**三个关键点：**

| 观察 | 意义 |
|---|---|
| `review.warn`：「2 条判据只有描述、没有硬检查…交给 judge 评估」 | **我上轮建议的"宽容降级"落地了**——不再整轮 degraded，而是降级到 judge-only 并显式告警 |
| `evidence:"操作系统（Operating System）"`、`"OS 作为系统内核…"` | **H2 的"答案原文窗口"在真实模型下也生效了** |
| 答案 252 字、含 2 个编号小标题、讲资源管理/接口抽象 | **不再是"抄指导者的句子"**——质量明显是 exec 自己写的 |

### 第 2、3 次：**unapproved（rounds=3）**

```
第2次: verdict{pass:false, failed:["c3"],
        items:[{criterion:"至少两个要点", check:"分点作答",
                evidence:"答案只有 1 个要点，至少需要 2 个"}]}
第3次: warn「1 条判据只有描述…」+ verdict{pass:false, failed:["c3"],
        evidence:"答案只有 1 个要点，至少需要 2 个"}
```

**两次都卡在同一条：`min_points ≥ 2` 被判定为"只有 1 个要点"。**

---

## 3. 🟠 J1：`min_points` 判定过粗，导致误杀

第 2 次的答案（163 字）：

```
**操作系统是什么及其基本功能：**
- 它是系统级软件，负责协调和管理硬件与软件的资源分配；它如何管理资源：通过进程调度、
  内存管理和中断处理等机制实现多任务运行和高效协作；它提供的服务及用户接口：…
```

答案里有 **3 个分号分隔的要点**、1 个 `-` 列表项，但 `min_points` 只数到 **1**。

**问题**：要点计数很可能是按"行首 `-`/`*`/`1.` 列表标记"来数，而中文答案常用**分号/顿号行内枚举**，直接被算作 1 个。

**建议**：
1. 计数规则放宽：同时识别 `^\s*[-*+]\s`、`^\s*\d+[.)]\s`、`**加粗小标题**`、以及**句末分号/换行分段**；
2. 或把 `min_points` 改成**在 guide 侧明确要求"答案必须用编号列表"**，让判据与形态匹配；
3. 更稳妥：`min_points` 这类"形态判据"**失败时不应直接判死**，应像 `review.warn` 那样降级给 judge 复核。

> 现状是"格式被害"：内容其实够，但形式不匹配判据，被判 3 轮不合格。

---

## 4. 🟠 J2：exec 把 rubric 编号写进答案

第 3 次的答案：

```
操作系统是管理计算机系统资源的软件核心。**c1.**
**它负责调度 CPU、内存和存储等硬件资源**，并协调各种应用程序的运行。**c2:**
- **操作系统的内核充当用户程序与底层硬件的接口层**。-**c3. 提供文件系统服务（管理和存取数据）**。
```

`c1.` / `c2:` / `c3.` 是 **rubric 条目的 id**，被模型当成了"必须出现的小标题"抄进答案。

**根因推断**：`review.feedback.text` 用「`- c1: …`」这种**裸 id + 冒号**的格式：

```
上一版未通过三段校验。请只修正下列未通过项…
- c1: 判据=「…」；核对=…；未通过证据=…
- c2: 判据=「…」；核对=…；未通过证据=…
```

模型把 `c1:` 理解成"答案里要有的标记"。

**建议**：
1. feedback 里改用**自然语言标签**（"第 1 项：…"）或加分隔符（`〔1〕`），**避免裸 `cN:` 前缀**；
2. 或在 feedback 末尾明确一句：「不要把这些编号写进答案」。

> 这个副作用在 rc.2.1 没出现、rc.3.1 出现了——因为 rc.3.1 的 feedback 带上了 `criterion/check`（更详细），
> 也更容易被模型误当成模板。

---

## 5. S3 回归

```
触发 5/5 · 工具名命中 5/5 · 执行成功 3/5 · 预期失败 1 · 真失败 1/5 · 答复 4/5
写入校验 workspace/w1.txt : 存在 -> 'HELLO_W_99'   ← ✅
```

- **触发与工具名保持 5/5**（rc.3 的 H4 改进稳定）；
- t5 正常调 `tool.fs.read`（10.0s）；
- t6（负样本）正确走 `tool.fs.read` → `预期失败` + 答复正确；
- 答复正确 4/5（本轮最好水平）。

---

## 6. ⚠️ bench：环境漂移，本轮数据不可用于版本对比

### 6.1 rc.3.1 连跑 3 次（空载）

| # | warm boot | 门禁 | cold spawn | write P95 | RPC P95 |
|---|---|---|---|---|---|
| 1 | 108.3 ms | ❌ false | 225.4 ms | 10,398 µs | 1.406 ms |
| 2 | 116.5 ms | ❌ false | 233.2 ms | 9,788 µs | 1.551 ms |
| 3 | 314.4 ms | ❌ false | 223.8 ms | 9,559 µs | 0.874 ms |

**多项门禁同时变红**（boot / write / rpc）。

### 6.2 对照实验：同一时刻跑 rc.3

| # | warm boot | 门禁 | cold spawn | write P95 | RPC P95 |
|---|---|---|---|---|---|
| 1 | 107.9 ms | ❌ false | 164.0 ms | 8,606 µs | 0.883 ms |
| 2 | 118.9 ms | ❌ false | 249.9 ms | 10,740 µs | 1.243 ms |

**rc.3 今早实测是 45.6 ms（7/7 全绿），现在也是 108~119 ms。**

→ **结论：整机进入降级状态（长时间测试后的系统负载/磁盘/实时防护），与本版本无关。**
→ **本轮 bench 数据不可用于对比 rc.2.1/rc.3 的历史数字。**

### 6.3 建议

1. **bench 报告里加一条环境指纹**（CPU 负载、是否刚跑过重任务、可用内存），否则数字无法横向比较；
2. **门禁判定建议用"多次取中位 + 丢弃首个样本"**（warm boot 已有 `boot_warmup_discarded_ms`，但 write/rpc 没有类似机制）；
3. 发版前的门禁验证应在一台**刚重启的干净机器**上做一次，作为基线存档。

---

## 7. 问题清单

| # | 严重度 | 问题 | 建议 |
|---|---|---|---|
| **J1** | 🟠 | `min_points` 只按列表标记计数，中文分号枚举算 1 个 → 2/3 轮误杀 | 放宽计数规则；形态判据失败时降级给 judge 复核 |
| **J2** | 🟠 | exec 把 `c1/c2/c3` 编号抄进答案（feedback 用裸 `cN:` 前缀导致） | feedback 改自然语言标签，并明确"编号不要写进答案" |
| J3 | 🟡 | bench 缺环境指纹，数字跨时段不可比 | 报告里带负载/内存快照 |
| — | ✅ | ~~I1 真实模型全降级~~ | **已修**（1/3 approved） |
| — | ✅ | ~~I2 guide 拿不到 schema~~ | **已修**（guide 一次成功） |
| — | ✅ | ~~宽容降级~~ | **已实现**（`review.warn` + judge-only） |

---

## 8. 未验证项

| 项 | 说明 |
|---|---|
| `plugin.settings` 的模型下拉（`provider.models` 驱动） | 需 UI 点击 |
| TUI 插件页"任意字段 Enter 保存" | 需 PTY 操作 |
| 权限提示显示完整命令 | 需触发 dangerous 工具并截图 |
| 产品显示名改为 **FeatherAgent** | 未专门核对（banner 仍打印 `featheragent 1.2.1-rc.3.1`） |
| H3 未知工具纠错 | 未触发到 |
| 五个 OpenAI 兼容 provider、Bearer 注入、网络盘、24h、高 DPI | 未做 |

---

## 9. 复现命令

```powershell
cd FeatherAgent-1.2.1-rc.3.1-windows-x64
$env:HTTP_PROXY="http://127.0.0.1:34686"; Remove-Item Env:NO_PROXY -ErrorAction SilentlyContinue

.\featheragent.exe serve --root . --provider provider-ollama --model qwen3.5:4b
.\featheragent-webui.exe --root .
# config.set: guide/judge.model=qwen2.5-coder:7b, review_gate.enabled=true

1..3 | % { .\featheragent.exe chat --root . --session c$_ --text "用三句话说明什么是操作系统" }
# 期望看到 review.warn（描述型判据降级）与 review.verdict（含答案原文 evidence）
```

---

## 10. 演进总览

| 指标 | rc.2.1 | rc.3 | **rc.3.1** |
|---|---|---|---|
| review-gate 真实模型 | 2~3 轮 approved（抄句子） | 🔴 4/4 degraded | **1/3 approved · 2/3 unapproved** |
| 判据类型 | 仅 `must_include` 整句 | 4 类（但产不出） | **4 类 + 描述型降级** |
| evidence 质量 | 提示词回显 | 答案原文窗口（mock） | **答案原文窗口（真机）** |
| 答案质量 | 抄指导者句子 | 未走到执行 | **252 字结构化，自主生成** |
| 显式告警 | 无 | 无 | **`review.warn`** |
| S3 答复 | 3/5 | 3/5 | **4/5** |
| 阻断级缺陷 | 0 | 0 | **0** |

> **这版是 review-gate 第一次"像个质量门"**：
> 它会拒绝（2/3 unapproved）、会解释（evidence 是答案原文）、会在判据不硬时降级告警而不是摆烂。
>
> 剩下的 J1/J2 都是"措辞与计数"层面的细节，不影响架构。
> 修掉 `min_points` 的计数规则与 feedback 的 `cN:` 前缀，approve 率应该能到 2/3~3/3。
