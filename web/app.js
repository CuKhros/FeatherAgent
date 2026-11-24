// FeatherAgent WebUI：默认 WebSocket（同连接 RPC + 事件推送），失败回退 HTTP + SSE。
// token 从 URL fragment 读取后立刻从地址栏抹掉（不进 Referer、不进历史标题）。

(() => {
  const $ = (s) => document.querySelector(s);
  const PROVIDER_DEFAULT_BASE = {
    "provider-ollama": "http://127.0.0.1:11434",
    "provider-llamacpp": "http://127.0.0.1:8080",
    "provider-ninfer": "http://127.0.0.1:8080",
    "provider-lmstudio": "http://127.0.0.1:1234",
    "provider-vllm": "http://127.0.0.1:8000",
    "provider-openai": "http://127.0.0.1:8080",
  };
  const DEFAULT_BASE_SET = new Set(Object.values(PROVIDER_DEFAULT_BASE));

  let token = "";
  {
    const m = location.hash.match(/token=([0-9a-fA-F]+)/);
    if (m) token = m[1];
    if (token) history.replaceState(null, "", location.pathname + location.search);
  }
  if (!token) {
    token = (prompt("请输入终端输出的 WebUI token（URL # 后的部分）") || "").trim();
    if (!token) {
      $("#conn").textContent = "缺少 token，无法连接";
      return;
    }
  }

  let rpcId = 1;
  let current = null;
  let sessions = [];
  let pendingConfirm = null;
  let ws = null;
  let cfg = null;
  let providers = [];
  let cfgModels = [];
  let pluginData = { registry: [], live: [] };
  let pluginDetailName = "";
  let workspaces = [];
  let curState = "idle";
  const pending = new Map();

  function esc(s) {
    return String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
  }

  async function httpRpc(method, params = {}) {
    const res = await fetch("/rpc", {
      method: "POST",
      headers: { "Content-Type": "application/json", "Authorization": "Bearer " + token },
      body: JSON.stringify({ jsonrpc: "2.0", id: rpcId++, method, params }),
    });
    if (res.status === 401) throw new Error("token 无效");
    const j = await res.json();
    if (j.error) throw new Error(j.error.message || "RPC 错误");
    return j.result;
  }

  function wsRpc(method, params = {}, timeoutMs = 30000) {
    return new Promise((resolve, reject) => {
      const id = rpcId++;
      const timer = setTimeout(() => {
        pending.delete(id);
        reject(new Error(`RPC 超时：${method}`));
      }, timeoutMs);
      pending.set(id, { resolve, reject, timer });
      try {
        ws.send(JSON.stringify({ jsonrpc: "2.0", id, method, params }));
      } catch (e) {
        clearTimeout(timer);
        pending.delete(id);
        reject(e);
      }
    });
  }

  async function rpc(method, params = {}, timeoutMs = 30000) {
    if (ws && ws.readyState === WebSocket.OPEN) return wsRpc(method, params, timeoutMs);
    return httpRpc(method, params);
  }

  function setState(state) {
    curState = state || "idle";
    $("#state-label").textContent = state;
    const dot = $("#state-dot");
    dot.className = "dot " + (state === "running" || state === "generating" ? "running" : state === "cancelled" ? "cancelled" : "idle");
    $("#session-label").textContent = current ? "会话 " + current : "未选择会话";
  }

  function mdInline(src) {
    let t = esc(src);
    t = t.replace(/`([^`]+)`/g, '<code class="inline">$1</code>');
    t = t.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
    t = t.replace(/\*([^*]+)\*/g, "<em>$1</em>");
    t = t.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (m, label, url) => {
      if (!/^https?:\/\//i.test(url)) return m;
      return `<a href="${url}" target="_blank" rel="noopener noreferrer">${label}</a>`;
    });
    return t;
  }

  function renderMarkdown(text) {
    const lines = String(text || "").replace(/\r/g, "").split("\n");
    const out = [];
    let para = [];
    let list = null;
    let inCode = false;
    const flushPara = () => {
      if (para.length) {
        out.push("<p>" + para.map(mdInline).join("<br>") + "</p>");
        para = [];
      }
    };
    const closeList = () => {
      if (list) { out.push(`</${list}>`); list = null; }
    };
    const ensureList = (kind) => {
      flushPara();
      if (list !== kind) { closeList(); out.push(`<${kind}>`); list = kind; }
    };
    for (const raw of lines) {
      const line = raw.replace(/\s+$/g, "");
      if (/^```/.test(line.trim())) {
        flushPara(); closeList();
        if (!inCode) {
          const lang = line.trim().slice(3).trim();
          out.push(`<pre class="code">${lang ? `<code class="lang-${esc(lang)}">` : "<code>"}`);
          inCode = true;
        } else {
          out.push("</code></pre>");
          inCode = false;
        }
        continue;
      }
      if (inCode) { out.push(esc(raw) + "\n"); continue; }
      if (!line.trim()) { flushPara(); closeList(); continue; }
      const h = line.match(/^(#{1,6})\s+(.*)$/);
      if (h) {
        flushPara(); closeList();
        out.push(`<h${h[1].length}>${mdInline(h[2])}</h${h[1].length}>`);
        continue;
      }
      if (/^\s*([-*_])\1\1+\s*$/.test(line)) {
        flushPara(); closeList(); out.push("<hr>"); continue;
      }
      const ul = line.match(/^\s*[-*+]\s+(.*)$/);
      if (ul) { ensureList("ul"); out.push(`<li>${mdInline(ul[1])}</li>`); continue; }
      const ol = line.match(/^\s*\d+[.)]\s+(.*)$/);
      if (ol) { ensureList("ol"); out.push(`<li>${mdInline(ol[1])}</li>`); continue; }
      const quote = line.match(/^\s*>\s?(.*)$/);
      if (quote) {
        flushPara(); closeList();
        out.push(`<blockquote>${mdInline(quote[1])}</blockquote>`);
        continue;
      }
      para.push(line);
    }
    flushPara(); closeList();
    if (inCode) out.push("</code></pre>");
    return out.length ? out.join("") : esc(text);
  }

  function addMessage(role, text) {
    if (!text) return;
    const markdown = role === "assistant";
    const div = document.createElement("div");
    div.className = "msg " + role + (markdown ? " md" : "");
    div.innerHTML = `<span class="role">${esc(role)}</span>${markdown ? renderMarkdown(text) : esc(text)}`;
    $("#messages").appendChild(div);
    $("#messages").scrollTop = $("#messages").scrollHeight;
  }

  function renderSessions() {
    const box = $("#session-list");
    box.innerHTML = "";
    for (const s of sessions) {
      const div = document.createElement("div");
      div.className = "session" + (s.id === current ? " active" : "");
      const live = s.live && s.live.state ? s.live.state : "history";
      div.innerHTML = `<div class="sid">${esc(s.id)}</div><div class="smeta">${esc(s.workspace || "default")} · ${s.records || 0} 条 · ${esc(live)}</div>`;
      div.onclick = () => resume(s.id);
      box.appendChild(div);
    }
  }

  async function refreshSessions() {
    try {
      const v = await rpc("session.list");
      sessions = v.sessions || [];
      renderSessions();
    } catch (e) {
      $("#conn").textContent = "内核不可用：" + e.message;
    }
  }

  async function refreshCtx() {
    if (!current) return;
    try {
      const v = await rpc("ctx.inspect", { session: current });
      const snap = v.snapshot || {};
      const st = snap.stats || {};
      const world = (cfg && cfg.context_window) || 8192;
      const total = st.total_tokens || 0;
      const pct = Math.min(100, Math.round((total / world) * 100));
      const layers = st.layers || {};
      const rows = [
        `<span class="k">预算</span> ${total} / ${world} tokens (${pct}%)<div class="bar"><i style="width:${pct}%"></i></div>`,
        `<span class="k">L1</span> ${layers.l1_window_messages || 0} 条 · <span class="k">L2</span> ${layers.l2_summary_cards || 0} 卡 · <span class="k">L3</span> ${layers.l3_task_state_tokens || 0} tok · <span class="k">L4</span> ${layers.l4_recalled_items || 0} 项`,
        `<span class="k">system</span> ${st.system_tokens || 0} · <span class="k">tools</span> ${st.tools_tokens || 0} · <span class="k">skills</span> ${st.skills_tokens || 0}`,
        `<span class="k">history</span> ${st.history_tokens || 0} · <span class="k">summary</span> ${st.summary_tokens || 0} · <span class="k">recall</span> ${st.recall_tokens || 0}`,
        `<span class="k">模型</span> ${esc(snap.model || "-")} · <span class="k">事件</span> ${snap.turns || 0} · <span class="k">回合</span> ${v.turn || 0}`,
      ];
      const goal = snap.task_state && snap.task_state.goal;
      if (goal) rows.push(`<span class="k">目标</span> ${esc(goal)}`);
      $("#ctx").innerHTML = rows.join("<br>");
      setState(v.state || "idle");
    } catch (e) {
      $("#ctx").textContent = "读取快照失败：" + e.message;
    }
  }

  function replayEvents(events) {
    for (const e of events || []) {
      const p = e.payload || {};
      if (e.kind === "user.message") {
        if (p.source === "steer") addMessage("user steer", "引导：" + (p.text || ""));
        else addMessage("user", p.text || "");
      } else if (e.kind === "model.message" && p.text) {
        addMessage("assistant", p.text);
      } else if (e.kind === "tool.result") {
        const r = p.result || {};
        addMessage("tool", `${p.name || "tool"} → ${r.status || "ok"}${r.content ? "\n" + String(r.content).slice(0, 1200) : ""}`);
      } else if (e.kind === "tool.error") {
        addMessage("system", `工具失败：${p.error || p.name || ""}`);
      } else if (e.kind === "context.compacted" && p.summary) {
        addMessage("system", "上下文已压缩：" + p.summary);
      }
    }
  }

  async function resume(id) {
    try {
      const v = await rpc("session.resume", { session: id });
      current = id;
      $("#messages").innerHTML = "";
      const ws = (v && v.workspace) || "default";
      addMessage("system", `已恢复会话 ${id}｜项目 ${ws}｜共 ${v.total || 0} 条事件`);
      replayEvents(v.events);
      if ([...$("#new-workspace").options].some((o) => o.value === ws)) $("#new-workspace").value = ws;
      $("#hint").textContent = "";
      renderSessions();
      await refreshCtx();
    } catch (e) {
      $("#hint").textContent = "恢复失败：" + e.message;
    }
  }

  async function refreshWorkspaces() {
    try {
      const v = await rpc("workspace.list");
      workspaces = v.workspaces || [];
      const sel = $("#new-workspace");
      const old = sel.value;
      sel.innerHTML = "";
      for (const w of workspaces) {
        const o = document.createElement("option");
        o.value = w.name;
        o.textContent = `${w.name}${w.has_agent ? " · 项目" : ""}`;
        sel.appendChild(o);
      }
      if (old && workspaces.some((w) => w.name === old)) sel.value = old;
      else if (workspaces.some((w) => w.name === "default")) sel.value = "default";
      if (!workspaces.length) {
        const o = document.createElement("option");
        o.value = "default";
        o.textContent = "default";
        sel.appendChild(o);
      }
    } catch (e) {
      $("#hint").textContent = "读取项目工作区失败：" + e.message;
    }
  }

  async function createWorkspace() {
    const name = (prompt("新建项目工作区名（将创建在 workspace_root 下一层）", "") || "").trim();
    if (!name) return;
    try {
      await rpc("workspace.create", { name });
      await refreshWorkspaces();
      $("#new-workspace").value = name;
      $("#hint").textContent = `已创建项目工作区 ${name}，含独立 .featheagent/`;
    } catch (e) {
      $("#hint").textContent = "创建项目失败：" + e.message;
    }
  }

  async function createSession() {
    const id = ($("#new-id").value || "").trim() || "s-webui-" + Date.now().toString(36);
    const workspace = ($("#new-workspace").value || "default").trim();
    try {
      await rpc("session.create", { id, workspace });
      $("#new-id").value = "";
      await refreshSessions();
      await resume(id);
      addMessage("system", "新会话 " + id);
    } catch (e) {
      $("#hint").textContent = "创建失败：" + e.message;
    }
  }

  async function send() {
    if (!current) {
      $("#hint").textContent = "先新建或选择会话";
      return;
    }
    const box = $("#input");
    const text = box.value.trim();
    if (!text) return;
    box.value = "";
    const running = ["running", "assembling", "generating", "tool_executing"].includes(curState);
    addMessage(running ? "user steer" : "user", running ? "引导：" + text : text);
    try {
      if (running) {
        await rpc("chat.inject", { session: current, text });
        $("#hint").textContent = "引导已插入上下文；当前生成会在边界中断后继续";
      } else {
        await rpc("chat.send", { session: current, text });
        setState("running");
        $("#hint").textContent = "";
      }
    } catch (e) {
      addMessage("system", "发送失败：" + e.message);
      $("#hint").textContent = e.message;
    }
  }

  async function cancel() {
    if (!current) return;
    try {
      await rpc("chat.stop", { session: current });
      $("#hint").textContent = "已紧急停止；已生成内容与上下文保留在会话日志";
    } catch (e) {
      $("#hint").textContent = "取消失败：" + e.message;
    }
  }

  async function compact() {
    if (!current) return;
    $("#hint").textContent = "正在生成上下文摘要…";
    try {
      const v = await rpc("context.compact", { session: current }, 200000);
      addMessage("system", `已压缩上下文：${v.compacted_events} 个旧事件 → ${v.summary_chars} 字符摘要`);
      $("#hint").textContent = "压缩完成；旧原文仍保留在会话日志";
      await refreshCtx();
    } catch (e) {
      $("#hint").textContent = "压缩失败：" + e.message;
    }
  }

  async function fork() {
    if (!current) return;
    const target = (prompt("fork 为（空则自动）", current + "-fork") || "").trim();
    try {
      const v = await rpc("session.fork", { session: current, as: target || current + "-fork" });
      addMessage("system", `已 fork，复制 ${v.copied} 条事件`);
      await refreshSessions();
    } catch (e) {
      $("#hint").textContent = "fork 失败：" + e.message;
    }
  }

  function showConfirm(params) {
    pendingConfirm = params.token;
    $("#modal-tool").textContent = `${params.tool}（风险：${params.risk}）`;
    $("#modal").classList.remove("hidden");
  }

  async function answerConfirm(allow) {
    const token2 = pendingConfirm;
    pendingConfirm = null;
    $("#modal").classList.add("hidden");
    if (!token2) return;
    try {
      await rpc(allow ? "tool.confirm" : "tool.deny", { token: token2, allow });
      addMessage("system", allow ? "已允许危险工具" : "已拒绝危险工具");
    } catch (e) {
      addMessage("system", "确认失败：" + e.message);
    }
  }

  function handleNotification(v) {
    if (v.method === "tool.confirm_request") { showConfirm(v.params || {}); return; }
    if (v.method !== "event") return;
    const k = v.params && v.params.kind;
    const p = (v.params && v.params.payload) || {};
    if (p.session && p.session !== current && p.session !== "_system") return;
    switch (k) {
      case "model.message": if (p.text) addMessage("assistant", p.text); break;
      case "user.message":
        if (p.source === "steer") addMessage("user steer", "引导：" + (p.text || ""));
        break;
      case "context.compacted": {
        const n = p.compacted_events || 0;
        addMessage("system", `上下文已压缩：${n} 个旧事件 → ${p.after_chars || 0} 字符摘要（原文仍在日志）`);
        refreshCtx();
        break;
      }
      case "tool.requested": addMessage("tool", `请求 ${p.tool || "?"}`); break;
      case "tool.result": {
        let brief = "";
        try { brief = JSON.stringify(p.result); } catch { brief = String(p.result); }
        addMessage("tool", `${p.name || "?"} → ${brief.slice(0, 400)}`);
        break;
      }
      case "tool.error": addMessage("tool", `${p.name || "?"} 业务错误: ${p.error || ""}`); break;
      case "tool.failed": addMessage("tool", `${p.name || "?"} 插件失败: ${p.error || ""}`); break;
      case "tool.degraded": addMessage("system", `工具熔断：${p.name || "?"}（连续 ${p.failures || 3} 次技术失败）`); break;
      case "review.guide": addMessage("system", `三段校验：已生成 ${p.count || 0} 条验收项`); break;
      case "review.verdict":
        addMessage("system", p.pass
          ? `三段校验第 ${p.round || 1} 轮：通过`
          : `三段校验第 ${p.round || 1} 轮：未通过（${(p.failed || []).join(", ")}）`);
        break;
      case "review.finished":
        if (p.status === "degraded") addMessage("system", `三段校验降级：${p.reason || "无法完成"}，已返回最后一版`);
        else if (p.status === "unapproved") addMessage("system", "三段校验未批准：已达修订上限，返回最后一版（unapproved）");
        break;
      case "config.changed": addMessage("system", "内核配置已更新"); break;
      case "permission.denied": addMessage("system", "拒绝：" + (p.reason || "")); break;
      case "plugin.degraded": addMessage("system", "插件降级（连续失败 3 次）"); break;
      case "subscriber.lagged": addMessage("system", "订阅落后，已丢弃旧事件"); break;
      case "memory.recalled": if ((p.items || []).length) addMessage("system", `召回 ${p.items.length} 条记忆`); break;
      case "chat.finished":
        addMessage("system", `— 本轮结束（${p.status || "done"}）`);
        setState((p.status === "cancelled" || p.status === "stopped") ? "cancelled" : "idle");
        refreshCtx();
        refreshSessions();
        break;
      default: break;
    }
  }

  function setSettingsStatus(text, kind) {
    const el = $("#settings-status");
    el.textContent = text || "";
    el.style.color = kind === "err" ? "var(--danger)" : kind === "ok" ? "var(--accent)" : "var(--muted)";
  }

  function syncTemperature() {
    const n = Number($("#cfg-temperature").value || 0);
    $("#cfg-temperature-val").textContent = n.toFixed(2);
  }

  function syncModelSelect() {
    const sel = $("#cfg-model-select");
    if (!sel) return;
    const current = ($("#cfg-model").value || "").trim();
    sel.value = cfgModels.includes(current) ? current : "";
  }

  function renderModelList() {
    const sel = $("#cfg-model-select");
    if (!sel) return;
    sel.innerHTML = "";
    const ph = document.createElement("option");
    ph.value = "";
    ph.textContent = cfgModels.length
      ? `选择已发现模型（${cfgModels.length}）`
      : "未发现模型（可手填）";
    sel.appendChild(ph);
    // 原生 datalist 会按当前输入值过滤，导致只显示一项；这里用真正的 select 列全量。
    for (const m of cfgModels) {
      const o = document.createElement("option");
      o.value = m;
      o.textContent = m;
      sel.appendChild(o);
    }
    syncModelSelect();
  }

  async function loadProviders() {
    const v = await rpc("plugin.list");
    const names = new Set();
    const add = (x) => {
      if (!x || !x.name) return;
      if ((x.category || "").includes("provider") || x.name.startsWith("provider-")) names.add(x.name);
    };
    for (const x of (v.registry || [])) add(x);
    for (const x of (v.live || [])) add(x);
    providers = [...names].sort();
    return providers;
  }

  async function refreshModels() {
    const provider = $("#cfg-provider").value;
    const baseUrl = ($("#cfg-base-url").value || "").trim();
    const model = ($("#cfg-model").value || "").trim();
    const apiKey = ($("#cfg-provider-key")?.value || "").trim();
    $("#cfg-models-hint").textContent = "正在读取模型列表…";
    try {
      const v = await rpc("provider.models", { provider, base_url: baseUrl, model, api_key: apiKey }, 45000);
      cfgModels = [...new Set(Array.isArray(v.models) ? v.models : [])].sort();
      renderModelList();
      if (v.error) {
        $("#cfg-models-hint").textContent = "无法读取模型列表（可手动填写模型名）";
        setSettingsStatus("模型列表：" + v.error, "warn");
      } else {
        $("#cfg-models-hint").textContent = `${cfgModels.length} 个模型，可直接输入或从列表选择`;
      }
    } catch (e) {
      cfgModels = [];
      renderModelList();
      $("#cfg-models-hint").textContent = "无法读取模型列表（可手动填写模型名）";
      setSettingsStatus("模型列表失败：" + e.message, "err");
    }
  }

  function setPluginStatus(text, kind) {
    const el = $("#plugin-status");
    if (!el) return;
    el.textContent = text || "";
    el.style.color = kind === "err" ? "var(--danger)" : kind === "ok" ? "var(--accent)" : "var(--muted)";
  }

  function pluginLive(name) {
    return (pluginData.live || []).find((x) => x.name === name) || {};
  }

  function pluginRegistry(name) {
    return (pluginData.registry || []).find((x) => x.name === name) || null;
  }

  async function refreshPlugins() {
    const v = await rpc("plugin.list", {}, 15000);
    pluginData = { registry: v.registry || [], live: v.live || [] };
    renderPluginNav();
    renderPluginList();
  }

  function renderPluginNav() {
    const nav = $("#plugin-nav");
    if (!nav) return;
    nav.innerHTML = "";
    const regs = [...(pluginData.registry || [])].sort((a, b) => String(a.name).localeCompare(String(b.name)));
    if (!regs.length) {
      const d = document.createElement("div");
      d.className = "nav-foot";
      d.textContent = "尚无已登记插件";
      nav.appendChild(d);
      return;
    }
    for (const r of regs) {
      const b = document.createElement("button");
      b.className = "nav-item plugin-nav-item";
      b.textContent = `${r.enabled === false ? "○" : "●"} ${r.name}`;
      b.title = `${r.category || "plugin"} · ${r.enabled === false ? "已停用" : "已启用"}`;
      b.onclick = () => showPluginDetail(r.name);
      nav.appendChild(b);
    }
  }

  function renderPluginList() {
    const box = $("#plugin-list");
    if (!box) return;
    box.innerHTML = "";
    const regs = [...(pluginData.registry || [])].sort((a, b) => String(a.name).localeCompare(String(b.name)));
    if (!regs.length) {
      box.innerHTML = '<div class="plugin-empty">没有已登记插件。</div>';
      return;
    }
    for (const r of regs) {
      const live = pluginLive(r.name);
      const enabled = r.enabled !== false;
      const row = document.createElement("div");
      row.className = "plugin-row";
      const info = document.createElement("div");
      info.className = "plugin-info";
      info.innerHTML = `<div><strong>${esc(r.name)}</strong> <span class="tag">${esc(r.category || "plugin")}</span></div>
        <div class="plugin-meta">${enabled ? (live.state === "ready" ? "运行中" : "按需启动") : "已停用"} · ${esc((r.blake3 || "").slice(0, 12))}</div>
        <code>${esc(r.program || "")}</code>`;
      const acts = document.createElement("div");
      acts.className = "plugin-actions";
      const detail = document.createElement("button");
      detail.textContent = "详情";
      detail.onclick = () => showPluginDetail(r.name);
      const toggle = document.createElement("button");
      toggle.textContent = enabled ? "停用" : "启用";
      toggle.className = enabled ? "ghost" : "primary";
      toggle.onclick = () => togglePlugin(r.name, enabled);
      acts.appendChild(detail);
      acts.appendChild(toggle);
      row.appendChild(info);
      row.appendChild(acts);
      box.appendChild(row);
    }
  }

  async function togglePlugin(name, enabled) {
    setPluginStatus(`${enabled ? "正在停用" : "正在启用"} ${name}…`, "warn");
    try {
      await rpc(enabled ? "plugin.disable" : "plugin.enable", { name });
      setPluginStatus(`${name} 已${enabled ? "停用" : "启用"}（运行进程已同步）`, "ok");
      await refreshPlugins();
      if (pluginDetailName === name) showPluginDetail(name);
    } catch (e) {
      setPluginStatus(`操作失败：${e.message}`, "err");
    }
  }

  async function installPlugin() {
    const path = ($("#plugin-install-path").value || "").trim();
    if (!path) {
      setPluginStatus("请填写插件目录绝对路径", "err");
      return;
    }
    setPluginStatus("正在安装…", "warn");
    try {
      const v = await rpc("plugin.install", { path }, 30000);
      const name = (v.installed && v.installed.name) || path;
      setPluginStatus(`已安装 ${name}`, "ok");
      $("#plugin-install-path").value = "";
      await refreshPlugins();
    } catch (e) {
      setPluginStatus(`安装失败：${e.message}`, "err");
    }
  }

  function showSettingsView(name) {
    for (const el of document.querySelectorAll(".settings-view")) el.classList.add("hidden");
    const view = $(`#view-${name}`);
    if (view) view.classList.remove("hidden");
    for (const b of document.querySelectorAll(".settings-nav .nav-item[data-view]")) {
      b.classList.toggle("active", b.dataset.view === name);
    }
    $("#plugin-nav")?.querySelectorAll(".plugin-nav-item").forEach((b) => b.classList.remove("active"));
    const titles = { general: "设置", plugins: "插件管理", plugin: "插件详情" };
    $("#settings-title").textContent = titles[name] || "设置";
    const subtitles = {
      general: "保存后通过 config.set 提交给内核；工作区改动在下次启动 serve 时生效。",
      plugins: "插件来自内核登记表；安装会登记清单，启停会同步运行进程。",
      plugin: "此页由 plugin.list 生成；安装或卸载插件后刷新设置页即可增删侧栏。",
    };
    $("#settings-subtitle").textContent = subtitles[name] || subtitles.general;
    if (name !== "plugin") pluginDetailName = "";
  }

  function showPluginDetail(name) {
    const r = pluginRegistry(name);
    if (!r) return;
    const live = pluginLive(name);
    const m = live.manifest || {};
    pluginDetailName = name;
    showSettingsView("plugin");
    $("#settings-title").textContent = `插件 · ${name}`;
    $("#plugin-nav")?.querySelectorAll(".plugin-nav-item").forEach((b) => {
      b.classList.toggle("active", b.textContent.includes(name));
    });
    const enabled = r.enabled !== false;
    const caps = Array.isArray(m.capabilities) ? m.capabilities : [];
    const perms = Array.isArray(m.permissions) ? m.permissions : [];
    $("#plugin-detail").innerHTML = `
      <div class="settings-section">
        <div class="section-title">插件清单</div>
        <div class="plugin-detail-head">
          <div><h3>${esc(name)}</h3><p class="warn">${esc(r.category || "plugin")} · 版本 ${esc(m.version || "未运行/未读取")} · ${enabled ? "已启用" : "已停用"}</p></div>
          <button id="btn-plugin-detail-toggle" class="${enabled ? "ghost" : "primary"}">${enabled ? "停用" : "启用"}</button>
        </div>
        <div class="kv-grid">
          <div><span>API</span><code>${esc(m.api || "-")}</code></div>
          <div><span>live</span><code>${esc(live.state || "stopped")}</code></div>
          <div><span>capabilities</span><code>${esc(caps.join(", ") || "-")}</code></div>
          <div><span>permissions</span><code>${esc(perms.join(", ") || "-")}</code></div>
          <div class="wide"><span>program</span><code>${esc(r.program || "-")}</code></div>
          <div class="wide"><span>blake3</span><code>${esc(r.blake3 || "-")}</code></div>
        </div>
        <p class="form-hint">该侧栏页由内核 <code>plugin.list</code> 的登记表生成；安装/卸载后刷新设置页即可增删。</p>
      </div>`;
    $("#btn-plugin-detail-toggle").onclick = () => togglePlugin(name, enabled);
  }

  async function openSettings() {
    setSettingsStatus("正在读取配置…", "warn");
    $("#settings-page").classList.remove("hidden");
    $("#btn-settings-save").disabled = true;
    $("#btn-settings-refresh").disabled = true;
    try {
      cfg = await rpc("config.get", {}, 15000);
      providers = await loadProviders();
      const sel = $("#cfg-provider");
      sel.innerHTML = "";
      const names = providers.slice();
      if (cfg.provider_plugin && !names.includes(cfg.provider_plugin)) names.unshift(cfg.provider_plugin);
      for (const n of names) {
        const o = document.createElement("option");
        o.value = n;
        o.textContent = n;
        sel.appendChild(o);
      }
      sel.value = cfg.provider_plugin || "provider-mock";
      $("#cfg-base-url").value = cfg.base_url || "http://127.0.0.1:11434";
      $("#cfg-provider-key").value = cfg.provider_api_key || "";
      $("#cfg-model").value = cfg.model || "qwen2.5:7b";
      syncModelSelect();
      $("#cfg-temperature").value = String(cfg.temperature ?? 0.7);
      $("#cfg-context-window").value = String(cfg.context_window ?? 8192);
      $("#cfg-max-turns").value = String(cfg.max_turns ?? 6);
      $("#cfg-auto-allow-safe").checked = cfg.auto_allow_safe !== false;
      $("#cfg-workspace-root").value = cfg.workspace_root || "";
      $("#cfg-search-provider").value = cfg.search_provider || "local";
      $("#cfg-search-base-url").value = cfg.search_base_url || "https://api.deepseek.com";
      $("#cfg-search-key-env").value = cfg.search_api_key_env || "DEEPSEEK_API_KEY";
      $("#cfg-search-count").value = String(cfg.search_count ?? 5);
      const rg = cfg.review_gate || {};
      $("#cfg-review-enabled").checked = !!rg.enabled;
      $("#cfg-review-max-rounds").value = String(rg.max_rounds ?? 2);
      $("#cfg-review-guide-model").value = (rg.guide && rg.guide.model) || "";
      $("#cfg-review-guide-temp").value = String((rg.guide && rg.guide.temperature) ?? 0.3);
      $("#cfg-review-exec-model").value = (rg.exec && rg.exec.model) || "";
      $("#cfg-review-exec-temp").value = String((rg.exec && rg.exec.temperature) ?? 0.7);
      $("#cfg-review-judge-model").value = (rg.judge && rg.judge.model) || "";
      $("#cfg-review-judge-temp").value = String((rg.judge && rg.judge.temperature) ?? 0.1);
      try {
        const st = await rpc("search.status");
        if (st.provider === "local") {
          $("#search-status").textContent = st.configured
            ? `本地搜索可用：必应 / DuckDuckGo / Wikipedia / Hacker News / GitHub，免费且无需 Key`
            : "本地搜索插件未就绪，请检查插件管理页";
        } else if (st.provider === "none") {
          $("#search-status").textContent = "联网搜索已关闭；模型不会获得任何联网工具";
        } else {
          $("#search-status").textContent = st.configured
            ? `已配置：${st.key_env} 已设置；Search/Fetch 按成功请求计费（5 元/1000 次）`
            : `未配置：设置环境变量 ${st.key_env} 后重启 serve，或切换为本地搜索`;
        }
      } catch (e) {
        $("#search-status").textContent = "读取联网状态失败：" + e.message;
      }
      syncTemperature();
      try {
        await refreshPlugins();
      } catch (e) {
        setPluginStatus("读取插件失败：" + e.message, "err");
      }
      showSettingsView("general");
      setSettingsStatus("");
      $("#btn-settings-save").disabled = false;
      $("#btn-settings-refresh").disabled = false;
      await refreshModels();
    } catch (e) {
      $("#btn-settings-save").disabled = false;
      $("#btn-settings-refresh").disabled = false;
      setSettingsStatus("读取配置失败：" + e.message, "err");
    }
  }

  function closeSettings() {
    $("#settings-page").classList.add("hidden");
    pluginDetailName = "";
    showSettingsView("general");
  }

  async function saveSettings() {
    const model = ($("#cfg-model").value || "").trim();
    const baseUrl = ($("#cfg-base-url").value || "").trim();
    const temperature = Number($("#cfg-temperature").value || 0);
    const contextWindow = Number($("#cfg-context-window").value || 8192);
    const maxTurns = Number($("#cfg-max-turns").value || 6);
    if (!model || !baseUrl) {
      setSettingsStatus("模型名和 Base URL 不能为空", "err");
      return;
    }
    if (!Number.isFinite(temperature) || temperature < 0 || temperature > 2) {
      setSettingsStatus("温度必须在 0.00–2.00 之间", "err");
      return;
    }
    const reviewEnabled = $("#cfg-review-enabled").checked;
    const reviewRounds = Number($("#cfg-review-max-rounds").value || 2);
    const guideTemp = Number($("#cfg-review-guide-temp").value || 0.3);
    const execTemp = Number($("#cfg-review-exec-temp").value || 0.7);
    const judgeTemp = Number($("#cfg-review-judge-temp").value || 0.1);
    // 即使当前未启用，也先校验，避免把越界值写进 config.json 后再打开。
    if (!Number.isInteger(reviewRounds) || reviewRounds < 1 || reviewRounds > 5) {
      setSettingsStatus("三段校验轮次必须在 1–5 之间", "err");
      return;
    }
    if (![guideTemp, execTemp, judgeTemp].every(Number.isFinite)
        || guideTemp < 0.2 || guideTemp > 0.5
        || execTemp < 0.6 || execTemp > 1.5
        || judgeTemp < 0 || judgeTemp > 0.2) {
      setSettingsStatus("三段校验温度越界：guide 0.2–0.5 / exec 0.6–1.5 / judge 0.0–0.2", "err");
      return;
    }
    setSettingsStatus("正在提交给内核…", "warn");
    try {
      const v = await rpc("config.set", { patch: {
        provider_plugin: $("#cfg-provider").value,
        base_url: baseUrl,
        provider_api_key: ($("#cfg-provider-key").value || "").trim(),
        model,
        temperature,
        context_window: contextWindow,
        max_turns: maxTurns,
        workspace_root: ($("#cfg-workspace-root").value || "").trim(),
        auto_allow_safe: $("#cfg-auto-allow-safe").checked,
        search_provider: $("#cfg-search-provider").value,
        search_base_url: ($("#cfg-search-base-url").value || "").trim(),
        search_api_key_env: ($("#cfg-search-key-env").value || "").trim(),
        search_count: Number($("#cfg-search-count").value || 5),
        review_gate: {
          enabled: reviewEnabled,
          max_rounds: reviewRounds,
          guide: { model: ($("#cfg-review-guide-model").value || "").trim(), temperature: guideTemp },
          exec: { model: ($("#cfg-review-exec-model").value || "").trim(), temperature: execTemp },
          judge: { model: ($("#cfg-review-judge-model").value || "").trim(), temperature: judgeTemp },
          on_exhausted: "return_last_with_flag",
        },
      }});
      cfg = v.config || cfg;
      setSettingsStatus("已保存并应用，下一轮推理使用新配置", "ok");
      addMessage("system", `配置已应用：${cfg.provider_plugin} / ${cfg.model} / T=${cfg.temperature}`);
      // 上下文面板在下一轮 chat.finished 后刷新；此处不阻塞设置保存。
    } catch (e) {
      setSettingsStatus("保存失败：" + e.message, "err");
    }
  }

  function openWs() {
    return new Promise((resolve, reject) => {
      let settled = false;
      ws = new WebSocket(`ws://${location.host}/ws?token=${encodeURIComponent(token)}`);
      ws.onopen = () => { settled = true; resolve(); };
      ws.onerror = () => { if (!settled) reject(new Error("WS 连接失败")); };
      ws.onclose = () => {
        $("#conn").textContent = "WS 已断开（刷新页面重连）";
        for (const [, p] of pending) {
          clearTimeout(p.timer);
          p.reject(new Error("WS 已断开"));
        }
        pending.clear();
        ws = null;
      };
      ws.onmessage = (ev) => {
        let v;
        try { v = JSON.parse(ev.data); } catch { return; }
        if (v.id !== undefined && pending.has(v.id)) {
          const p = pending.get(v.id);
          pending.delete(v.id);
          clearTimeout(p.timer);
          if (v.error) p.reject(new Error(v.error.message || "RPC 错误"));
          else p.resolve(v.result);
          return;
        }
        handleNotification(v);
      };
      setTimeout(() => { if (!settled) reject(new Error("WS 握手超时")); }, 3000);
    });
  }

  function openSse() {
    const es = new EventSource("/events?token=" + encodeURIComponent(token));
    es.onopen = () => { $("#conn").textContent = "已连接内核（HTTP/SSE 回退）"; };
    es.onerror = () => { $("#conn").textContent = "SSE 重连中…"; };
    es.onmessage = (ev) => {
      let v;
      try { v = JSON.parse(ev.data); } catch { return; }
      handleNotification(v);
    };
  }

  function bindUi() {
    $("#btn-send").onclick = send;
    $("#btn-stop").onclick = cancel;
    $("#btn-compact").onclick = compact;
    $("#btn-fork").onclick = fork;
    $("#btn-new").onclick = createSession;
    $("#btn-new-workspace").onclick = createWorkspace;
    $("#btn-settings").onclick = openSettings;
    $("#btn-plugins").onclick = async () => {
      await openSettings();
      showSettingsView("plugins");
    };
    $("#btn-settings-close").onclick = closeSettings;
    $("#btn-settings-cancel").onclick = closeSettings;
    $("#btn-settings-save").onclick = saveSettings;
    $("#btn-settings-refresh").onclick = refreshModels;
    $("#btn-plugin-refresh").onclick = () => refreshPlugins().then(() => setPluginStatus("插件列表已刷新", "ok")).catch((e) => setPluginStatus("刷新失败：" + e.message, "err"));
    $("#btn-plugin-install").onclick = installPlugin;
    $("#plugin-install-path").addEventListener("keydown", (e) => { if (e.key === "Enter") installPlugin(); });
    for (const b of document.querySelectorAll(".settings-nav .nav-item[data-view]")) {
      b.onclick = () => showSettingsView(b.dataset.view);
    }
    $("#cfg-provider").onchange = () => {
      const next = $("#cfg-provider").value;
      const cur = ($("#cfg-base-url").value || "").trim();
      if ((!cur || DEFAULT_BASE_SET.has(cur)) && PROVIDER_DEFAULT_BASE[next]) {
        $("#cfg-base-url").value = PROVIDER_DEFAULT_BASE[next];
      }
      refreshModels();
    };
    $("#cfg-base-url").onchange = refreshModels;
    $("#cfg-temperature").oninput = syncTemperature;
    $("#cfg-model-select").onchange = () => {
      const v = $("#cfg-model-select").value;
      if (v) $("#cfg-model").value = v;
    };
    $("#cfg-model").addEventListener("input", syncModelSelect);
    $("#btn-allow").onclick = () => answerConfirm(true);
    $("#btn-deny").onclick = () => answerConfirm(false);
    $("#input").addEventListener("keydown", (e) => {
      if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); send(); }
    });
    $("#new-id").addEventListener("keydown", (e) => { if (e.key === "Enter") createSession(); });
    window.addEventListener("keydown", (e) => {
      if (e.key !== "Escape") return;
      if ($("#settings-page").classList.contains("hidden")) return;
      if (pluginDetailName) { showSettingsView("plugins"); return; }
      closeSettings();
    });
  }

  (async () => {
    bindUi();
    try {
      await openWs();
      $("#conn").textContent = "已连接内核（WebSocket）";
    } catch {
      openSse();
      $("#conn").textContent = "WebSocket 不可用，回退 HTTP/SSE";
    }
    try {
      await rpc("initialize");
      cfg = await rpc("config.get").catch(() => null);
      await rpc("event.subscribe", { kinds: [] });
      await refreshWorkspaces();
      await refreshSessions();
      if (sessions.length) await resume(sessions[0].id);
      else await createSession();
    } catch (e) {
      $("#conn").textContent = "初始化失败：" + e.message;
    }
  })();
})();
