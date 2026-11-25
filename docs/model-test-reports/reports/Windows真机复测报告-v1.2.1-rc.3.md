# FeatherAgent v1.2.1-rc.3 · Windows 真机复测报告

> 测试机：Windows x64（同前）· Ollama 0.32.9
> 复测时间：2026-11-24
> 被测包：`FeatherAgent-1.2.1-rc.3-windows-x64`
> 对照：`Windows真机复测报告-v1.2.1-rc.2.1.md`（H1 ～ H4）

---

## 1. 结论

**H1/H2/H3/H4 在机制层面全部落地，且用 `provider-mock` 能完整证明其正确。**
**但真实模型上的效果反而退步：`review-gate` 4/4 次全部 degraded——严格化后的 rubric schema，本地最强模型（7B）一次都没产出合规项。**

| 项 | rc.2.1 | rc.3 | 判定 |
|---|---|---|---|
| **H1** 判据限长 + 非子串判据 | 整句 `must_include` | ✅ **机制已实现** | mock 下 `must_include`+`must_not_include` 双判据生效 |
| **H2** evidence 必须是答案原文窗口 | 回显提示词 | ✅ **机制已实现** | mock 下 `evidence:"REVIEW-FINAL"` 正是答案片段 |
| **H3** 未知工具纠错 | 无 | ⬜ 未直接验证 | 需构造幻觉工具名，本轮未触发 |
| **H4** S3 t5/t6 要求单次工具调用 | t5 不调工具 | ✅ **t5 已修** | t5 从"(无)"变成 `tool.fs.read`；t6 仍调 2 个 |
| **G3** status 三态 | 已修 | ✅ **保持** | `chat.status=degraded`、`review.status=degraded` 正确上报 |
| **review-gate 真实模型** | 2~3 轮 approved（但空洞） | 🔴 **4/4 全 degraded** | 见 §2 |
| bench 门禁 | 7/7 | ✅ **7/7** | 见 §6 |

---

## 2. 🔴 review-gate 真实模型：4/4 全降级

### 2.1 现象（连跑 3 次 + 前面 1 次，共 4 次）

```
第1次: chat.status=degraded  review.status=degraded  rounds=0
        reason=guide 连续 2 次未产出合规 rubric：
               rpc -32000: 验收项 1 没有判据：需要 must_include / must_not_include / min_points / pattern 之一
第2次: 同上
第3次: 同上
```

**4 次尝试、4 次 degraded、原因完全一致**，`rounds` 恒为 0（连执行者都没跑）。

配置：`guide=qwen2.5-coder:7b`、`judge=qwen2.5-coder:7b`、`exec=qwen3.5:4b`——**本机能用的最强组合**。

### 2.2 好消息：G3 的 status 语义确实修好了

```
chat.finished {"status":"degraded",
  "review":{"approved":false,"degraded":true,"enabled":true,"rounds":0,"status":"degraded"}}
```

对比 rc.2 之前的"恒 `ok`"——**现在客户端只看 `status` 就能知道这轮没走完校验**。这条我上轮提的 G3 已闭环。

### 2.3 关键隔离：内核没问题，是模型跟不上 schema

同一台机器，同一内核，只把 provider 换成 `provider-mock`：

```
review.guide   {attempts:1, count:2, tokens_approx:60}

review.verdict round1 {pass:false, failed:["c1"],
  items:[{criterion:"最终答案命中 REVIEW-FINAL 标记",
          check:"答案出现『REVIEW-FINAL』",
          evidence:"答案未逐字出现必须短语『REVIEW-FINAL』", pass:false}]}

review.feedback{"- c1: 判据=「最终答案命中 REVIEW-FINAL 标记」；核对=答案出现『REVIEW-FINAL』；未通过证据=…"}

review.verdict round2 {pass:true, failed:[],
  items:[{id:"c1", criterion:"最终答案命中 REVIEW-FINAL 标记", evidence:"REVIEW-FINAL", pass:true},
         {id:"c2", criterion:"答案不含被禁止的占位词",
          check:"不得出现『绝不应出现的占位词』", evidence:"REVIEW-FINAL", pass:true}]}

review.finished {rounds:2, status:"approved"}
chat.finished   {"status":"ok","review":{"approved":true,"degraded":false,"status":"approved"}}
```

**这张对照表把 H1/H2 全部证实了：**

| 验证点 | mock 下的证据 |
|---|---|
| `must_include` 硬检查 | `答案未逐字出现必须短语『REVIEW-FINAL』`（规则判定，非 LLM） |
| **`must_not_include` 也生效** | c2 判据是"答案不含被禁止的占位词"，`check:"不得出现『…』"` |
| **H2：evidence 是答案原文窗口** | round2 的 `evidence:"REVIEW-FINAL"` —— 正是答案片段，不再是提示词回显 |
| 多判据并存 | 一轮里同时出现 include + not_include 两类判据 |
| 2 轮批准 | `rounds:2, status:"approved"` |

→ **内核实现正确。真机上不工作，是因为 7B 模型产不出 `{must_include|…}` 这一层结构。**

### 2.4 根因推断

错误文案是「验收项 1 没有判据」，说明 **guide 产出了 items，但键名不对**——
很可能是沿用了 rc.2 时代的 `{criterion, check}` 形状（这两个字段在内核的 items 输出里**仍然存在**，只是不再是"判据"）。

也就是说：**内核把 `criterion/check` 保留成描述字段，却把真正的判据移到了 `must_include` 等新键上，而 guide 的 prompt 没有同步教会这件事。**

### 2.5 建议（按性价比）

| 优先 | 建议 | 理由 |
|---|---|---|
| **1** | **rubric 校验失败时，把 schema 错误回注给 guide 再重试**（现在重试了 2 次，但很可能只是原样重发） | 重发同样的 prompt 必然得到同样的错；`review.feedback` 对 exec 已经这么做了，对 guide 也应该 |
| **2** | **guide 的 prompt 里直接给 JSON schema + 一条 few-shot 示例**；或**由内核生成骨架**（只留值让模型填） | §11.4 就建议过"模板改成要求措辞，或内核生成骨架" |
| **3** | **宽容降级**：检测到 items 只有 `criterion/check` 而没有硬判据时，**把它当作"无硬检查"继续走 judge**，并打一条 `review.warn`，而不是整轮 degraded | 现在"要么严格要么全废"，中间态缺失 |
| **4** | UI 里明确写出"需 ≥7B 且能遵循 JSON schema" | START-HERE 已提示 ≥7B，但 7B 实测仍不合格，门槛应更高或给示例 |

> **发版判断**：rc.3 的 H1/H2 是"把尺子做准了"，但**尺子太复杂，本地模型量不出来**。
> 对比 rc.2.1（能批准但空洞）与 rc.3（严格但全废）——**两者都不理想，中间态才是答案**：
> **必须有一条判据，但允许多种判据类型；校验失败要教它怎么改，而不是直接判死。**

---

## 3. H3：未知工具纠错（未直接验证）

文档要求"显式回 '未知工具 + 可用工具' 纠正"。本轮模型没有自然产生畸形工具名
（rc.2.1 曾出现 `tool.fs`，本轮 S3 未复现），所以**该分支未被触发**。

建议：提供一个人为构造的手段（如 `chat --text` 里诱导，或单元测试里的 mock tool 名），否则每次都要靠模型心情。

---

## 4. H4：S3 的 t5/t6 单次工具调用

| 任务 | rc.2.1 | rc.3 |
|---|---|---|
| t5（读+总结） | **(无) 未触发** | ✅ **`tool.fs.read`**（1 个） |
| t6（读不存在文件） | `tool.fs, tool.shell×2`（畸形名） | `tool.fs.read, tool.fs.write`（**2 个**，仍多于 1） |

**t5 修好了**（这是 H4 的主要目标），t6 仍在多调一个 `tool.fs.write`。

---

## 5. S3 本轮数据

```
触发 5/5 · 工具名命中 5/5 · 执行成功 4/5 · 预期失败 0 · 真失败 1/5 · 答复 3/5
写入校验 workspace/w1.txt : 存在 -> 'HELLO_W_99'   ← 写盘正常
```

- **触发与工具名都回到 5/5**（rc.2.1 是 4/5、3/5），H4 的提示词改进有效；
- 唯一失分是 t6（负样本）多调了 `tool.fs.write`，导致 `ERR` 且答复未命中"不存在"；
- 写盘 ✅。

---

## 6. bench（7/7 全绿）

```
gates: assemble ✅ binary ✅ boot_lt_100ms ✅ cold_spawn_lt_250ms ✅
       rpc ✅ rss ✅ write_p95 ✅

binary 2.125 MB | warm boot 45.60 ms | cold spawn 193.68 ms
RSS 10,568 KB | write P95 3,988.8 µs | RPC P95 0.241 ms
```

与 rc.2.1 持平（warm 60→45 ms 属波动，均远低于门限）。

---

## 7. 插件设置声明化（新特性，未深测）

rc.3 的新架构：插件在 `plugin.json` 里声明 schema，内核经 `plugin.settings` 转发，TUI/WebUI 通用渲染。

这是**架构上的正确方向**（避免每个插件都在前端写死表单），但本轮未做 UI 点击验证。
建议补一条**契约测试**：给定 `plugin.json` 的 schema，`plugin.settings` 的返回必须能一一映射到渲染控件。

---

## 8. 问题清单

| # | 严重度 | 问题 | 建议 |
|---|---|---|---|
| **I1** | 🔴 | **review-gate 真实模型 4/4 degraded**：严格 rubric 让 7B guide 无法产出合规项 | 见 §2.5 四条，优先 1、3 |
| I2 | 🟠 | guide 重试很可能"原样重发"，没有把 schema 错误回注 | 把校验错误作为 feedback 回注给 guide |
| I3 | 🟡 | t6 仍多调一个工具（H4 未完全达成） | 提示词里加"只调用一个工具，禁止额外调用" |
| I4 | 🟡 | H3 未知工具纠错分支未验证到 | 提供可构造的测试入口 |
| — | ✅ | ~~H1 判据整句~~ | 机制已实现（判据 ≤20 字、非子串判据并存） |
| — | ✅ | ~~H2 evidence 回显~~ | 机制已实现（mock 下 evidence 为答案原文窗口） |
| — | ✅ | ~~G3 status 三态~~ | `degraded` 正确上报 |

---

## 9. 未验证项

| 项 | 说明 |
|---|---|
| TUI/WebUI 的 `plugin.settings` 通用渲染 | 需 UI 点击 |
| H3 未知工具纠错 | 未触发 |
| `unapproved` 态实拍（超轮次） | 本轮因 guide 失败未走到 |
| 五个 OpenAI 兼容 provider、Bearer 注入 | 未做 |
| 网络盘、24h、高 DPI、WebUI Markdown | 未做 |

---

## 10. 复现命令

```powershell
cd FeatherAgent-1.2.1-rc.3-windows-x64
$env:HTTP_PROXY="http://127.0.0.1:34686"; Remove-Item Env:NO_PROXY -ErrorAction SilentlyContinue

# 真实模型下 review-gate（应能复现 4/4 degraded）
.\featheragent.exe serve --root . --provider provider-ollama --model qwen3.5:4b
.\featheragent-webui.exe --root .
# config.set: guide/judge.model=qwen2.5-coder:7b, review_gate.enabled=true
.\featheragent.exe chat --root . --session r --text "用三句话说明什么是操作系统"
# -> review.finished{status:"degraded", reason:"验收项 1 没有判据…"}

# 对照：mock 下同一流程应 approved
.\featheragent.exe serve --root . --provider provider-mock
.\featheragent.exe chat --root . --session m --text "REVIEW_START"
# -> review.finished{rounds:2, status:"approved"}
```

---

## 11. 演进总览

| 指标 | rc.2 | rc.2.1 | **rc.3** |
|---|---|---|---|
| review-gate 机制 | mock 通过 | mock 通过 | **mock 通过 + 多判据类型** |
| review-gate 真实模型 | ❌ 3/3 不通过 | ⚠️ 能批准但空洞（抄句子） | 🔴 **4/4 全降级** |
| 判据质量 | judge 虚判 | `must_include` 整句 | **≤20 字 + 非子串判据** |
| evidence | `"总结"` | 提示词回显 | **答案原文窗口** |
| status 语义 | 恒 ok | 三态 | **三态（含 degraded）** |
| S3 触发 | 5/5 | 4/5 | **5/5** |
| bench | 7/7 | 7/7 | **7/7** |
| 阻断级缺陷 | 0 | 0 | **0** |

> **一句话总结这一版**：**引擎做对了，方向盘打过头了。**
>
> H1/H2 的实现质量很高（mock 两次运行把 include/not_include/原文窗口全证明了），
> 但"严格 schema"与"本地 7B 模型的能力"之间差了一截教学——**缺一条把校验错误讲给 guide 听的回路**。
>
> 修好 §2.5 的第 1 条（错误回注）大概率就能让真实模型重新跑通，因为 rc.2.1 已证明 7B
> 能产出结构化的 `criterion/check`，只是不知道要改用 `must_include`。
