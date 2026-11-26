// English WebUI bundle, maintained alongside web/app.js. Check with scripts/check-en-web.py.
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
    token = (prompt("Enter the WebUI token printed by the terminal (the part after # in the URL)") || "").trim();
    if (!token) {
      $("#conn").textContent = "Missing token; cannot connect";
      return;
    }
  }

  let rpcId = 1;
  let current = null;
  let sessions = [];
  let pendingConfirm = null;
  let attachments = [];
  let attaching = false;
  let ws = null;
  let cfg = null;
  let providers = [];
  let cfgModels = [];
  let pluginData = { registry: [], live: [] };
  let pluginDetailName = "";
  // 当前打开的Plugin settings声明：{name, root, fields, values}，由内核 plugin.settings 转述。
  let pluginSettings = null;
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
    if (res.status === 401) throw new Error("Invalid token");
    const j = await res.json();
    if (j.error) throw new Error(j.error.message || "RPC error");
    return j.result;
  }

  function wsRpc(method, params = {}, timeoutMs = 30000) {
    return new Promise((resolve, reject) => {
      const id = rpcId++;
      const timer = setTimeout(() => {
        pending.delete(id);
        reject(new Error(`RPC timeout: ${method}`));
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
    $("#session-label").textContent = current ? "Session " + current : "No session selected";
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
      div.innerHTML = `<div class="sid">${esc(s.id)}</div><div class="smeta">${esc(s.workspace || "default")} · ${s.records || 0} events · ${esc(live)}</div>`;
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
      $("#conn").textContent = "Kernel unavailable: " + e.message;
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
        `<span class="k">Budget</span> ${total} / ${world} tokens (${pct}%)<div class="bar"><i style="width:${pct}%"></i></div>`,
        `<span class="k">L1</span> ${layers.l1_window_messages || 0} items · <span class="k">L2</span> ${layers.l2_summary_cards || 0} cards · <span class="k">L3</span> ${layers.l3_task_state_tokens || 0} tok · <span class="k">L4</span> ${layers.l4_recalled_items || 0} items`,
        `<span class="k">system</span> ${st.system_tokens || 0} · <span class="k">tools</span> ${st.tools_tokens || 0} · <span class="k">skills</span> ${st.skills_tokens || 0}`,
        `<span class="k">history</span> ${st.history_tokens || 0} · <span class="k">summary</span> ${st.summary_tokens || 0} · <span class="k">recall</span> ${st.recall_tokens || 0}`,
        `<span class="k">Model</span> ${esc(snap.model || "-")} · <span class="k">Events</span> ${snap.turns || 0} · <span class="k">Turns</span> ${v.turn || 0}`,
      ];
      const goal = snap.task_state && snap.task_state.goal;
      if (goal) rows.push(`<span class="k">Goal</span> ${esc(goal)}`);
      $("#ctx").innerHTML = rows.join("<br>");
      setState(v.state || "idle");
    } catch (e) {
      $("#ctx").textContent = "Failed to read snapshot: " + e.message;
    }
  }

  function replayEvents(events) {
    for (const e of events || []) {
      const p = e.payload || {};
      if (e.kind === "user.message") {
        if (p.source === "steer") addMessage("user steer", "Steer: " + (p.text || ""));
        else addMessage("user", p.text || "");
      } else if (e.kind === "model.message" && p.text) {
        addMessage("assistant", p.text);
      } else if (e.kind === "tool.result") {
        const r = p.result || {};
        addMessage("tool", `${p.name || "tool"} → ${r.status || "ok"}${r.content ? "\n" + String(r.content).slice(0, 1200) : ""}`);
      } else if (e.kind === "tool.error") {
        addMessage("system", `Tool failed: ${p.error || p.name || ""}`);
      } else if (e.kind === "context.compacted" && p.summary) {
        addMessage("system", "Context compacted: " + p.summary);
      }
    }
  }

  async function resume(id) {
    try {
      const v = await rpc("session.resume", { session: id });
      current = id;
      attachments = [];
      renderAttachments();
      $("#messages").innerHTML = "";
      const ws = (v && v.workspace) || "default";
      addMessage("system", `Restored session ${id} | project ${ws} | ${v.total || 0} events`);
      replayEvents(v.events);
      if ([...$("#new-workspace").options].some((o) => o.value === ws)) $("#new-workspace").value = ws;
      $("#hint").textContent = "";
      renderSessions();
      await refreshCtx();
    } catch (e) {
      $("#hint").textContent = "Restore failed: " + e.message;
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
        o.textContent = `${w.name}${w.has_agent ? " · project" : ""}`;
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
      $("#hint").textContent = "Failed to read project workspace: " + e.message;
    }
  }

  async function createWorkspace() {
    const name = (prompt("New project workspace name (created one level under workspace_root)", "") || "").trim();
    if (!name) return;
    try {
      await rpc("workspace.create", { name });
      await refreshWorkspaces();
      $("#new-workspace").value = name;
      $("#hint").textContent = `Created project workspace ${name} with its own .featheagent/`;
    } catch (e) {
      $("#hint").textContent = "Project creation failed: " + e.message;
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
      addMessage("system", "New session " + id);
    } catch (e) {
      $("#hint").textContent = "Create failed: " + e.message;
    }
  }


  function renderAttachments() {
    const box = $("#attachments");
    box.innerHTML = "";
    for (const [i, a] of attachments.entries()) {
      const chip = document.createElement("span");
      chip.className = "attachment";
      chip.title = a.path || "";
      const name = document.createElement("span");
      name.textContent = `${a.name || "Attachment"} (${a.bytes || 0} B)`;
      const path = document.createElement("span");
      path.className = "apath";
      path.textContent = a.path || "";
      const del = document.createElement("button");
      del.type = "button";
      del.textContent = "×";
      del.onclick = () => { attachments.splice(i, 1); renderAttachments(); };
      chip.append(name, path, del);
      box.appendChild(chip);
    }
  }

  function readFileBase64(file) {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onerror = () => reject(new Error("Failed to read file"));
      reader.onload = () => {
        const s = String(reader.result || "");
        const comma = s.indexOf(",");
        if (comma < 0) reject(new Error("Failed to encode file"));
        else resolve(s.slice(comma + 1));
      };
      reader.readAsDataURL(file);
    });
  }

  async function attachFiles(files) {
    if (!files || !files.length || attaching) return;
    if (attachments.length + files.length > 8) {
      $("#hint").textContent = "At most 8 attachments at a time";
      return;
    }
    attaching = true;
    const btn = $("#btn-attach");
    if (btn) btn.disabled = true;
    let added = 0;
    for (const file of files) {
      if (file.size > 8 * 1024 * 1024) {
        $("#hint").textContent = `${file.name} exceeds 8 MiB and was not uploaded`;
        continue;
      }
      try {
        const data = await readFileBase64(file);
        const v = await httpRpc("file.attach", { name: file.name, data_base64: data }, 60000);
        attachments.push({ name: file.name, path: v.path, bytes: v.bytes || file.size });
        added += 1;
      } catch (e) {
        $("#hint").textContent = `Upload ${file.name} failed: ${e.message}`;
      }
    }
    attaching = false;
    if (btn) btn.disabled = false;
    renderAttachments();
    if (added) $("#hint").textContent = `Added ${added} attachment(s); sending will include the workspace path`;
  }

  function outgoingText(text) {
    let out = text || "";
    for (const a of attachments) {
      if (!out) out = "See the attached file.";
      out += `\n\n[Attachment] ${a.path}`;
    }
    return out;
  }

  async function send() {
    if (!current) {
      $("#hint").textContent = "Create or select a session first";
      return;
    }
    const box = $("#input");
    const text = box.value.trim();
    if (!text && !attachments.length) return;
    if (attaching) {
      $("#hint").textContent = "Attachment upload in progress; wait before sending";
      return;
    }
    const outgoing = outgoingText(text);
    const sentAttachments = attachments.slice();
    box.value = "";
    attachments = [];
    renderAttachments();
    const running = ["running", "assembling", "generating", "tool_executing"].includes(curState);
    addMessage(running ? "user steer" : "user", running ? "Steer: " + outgoing : outgoing);
    try {
      if (running) {
        await rpc("chat.inject", { session: current, text: outgoing });
        $("#hint").textContent = "Steer inserted; the current generation continues after a boundary interruption";
      } else {
        await rpc("chat.send", { session: current, text: outgoing });
        setState("running");
        $("#hint").textContent = "";
      }
    } catch (e) {
      attachments = sentAttachments;
      renderAttachments();
      addMessage("system", "Send failed: " + e.message);
      $("#hint").textContent = e.message;
    }
  }

  async function cancel() {
    if (!current) return;
    try {
      await rpc("chat.stop", { session: current });
      $("#hint").textContent = "Emergency stop complete; generated content and context remain in the session log";
    } catch (e) {
      $("#hint").textContent = "Cancel failed: " + e.message;
    }
  }

  async function compact() {
    if (!current) return;
    $("#hint").textContent = "Generating context summary…";
    try {
      const v = await rpc("context.compact", { session: current }, 200000);
      addMessage("system", `Context compacted: ${v.compacted_events} old events → ${v.summary_chars}-char summary`);
      $("#hint").textContent = "Compaction complete; original text remains in the session log";
      await refreshCtx();
    } catch (e) {
      $("#hint").textContent = "Compaction failed: " + e.message;
    }
  }

  async function fork() {
    if (!current) return;
    const target = (prompt("Fork name (empty = auto)", current + "-fork") || "").trim();
    try {
      const v = await rpc("session.fork", { session: current, as: target || current + "-fork" });
      addMessage("system", `Forked, copied ${v.copied} events`);
      await refreshSessions();
    } catch (e) {
      $("#hint").textContent = "Fork failed: " + e.message;
    }
  }

  function showConfirm(params) {
    pendingConfirm = params.token;
    $("#modal-tool").textContent = `${params.tool} (risk: ${params.risk})`;
    // 光有工具名没法判断：把内核送来的完整命令/参数摊开（rc.3 复测 I5）。
    let preview = params.preview;
    if (!preview && params.arguments !== undefined) {
      try { preview = JSON.stringify(params.arguments, null, 2); } catch { preview = String(params.arguments); }
    }
    $("#modal-preview").textContent = preview || "(no parameters provided by kernel)";
    $("#modal").classList.remove("hidden");
  }

  async function answerConfirm(allow) {
    const token2 = pendingConfirm;
    pendingConfirm = null;
    $("#modal").classList.add("hidden");
    if (!token2) return;
    try {
      await rpc(allow ? "tool.confirm" : "tool.deny", { token: token2, allow });
      addMessage("system", allow ? "Dangerous tool allowed" : "Dangerous tool denied");
    } catch (e) {
      addMessage("system", "Confirmation failed: " + e.message);
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
        if (p.source === "steer") addMessage("user steer", "Steer: " + (p.text || ""));
        break;
      case "context.compacted": {
        const n = p.compacted_events || 0;
        addMessage("system", `Context compacted: ${n} old events → ${p.after_chars || 0}-char summary (original text remains in the log)`);
        refreshCtx();
        break;
      }
      case "tool.requested": addMessage("tool", `Request ${p.tool || "?"}`); break;
      case "tool.result": {
        let brief = "";
        try { brief = JSON.stringify(p.result); } catch { brief = String(p.result); }
        addMessage("tool", `${p.name || "?"} → ${brief.slice(0, 400)}`);
        break;
      }
      case "tool.error": addMessage("tool", `${p.name || "?"} business error: ${p.error || ""}`); break;
      case "tool.failed": addMessage("tool", `${p.name || "?"} plugin failure: ${p.error || ""}`); break;
      case "tool.degraded": addMessage("system", `Tool circuit-breaker: ${p.name || "?"} (${p.failures || 3} consecutive technical failures)`); break;
      case "review.guide": addMessage("system", `Three-stage review: generated ${p.count || 0} acceptance items`); break;
      case "review.verdict":
        addMessage("system", p.pass
          ? `Three-stage review round ${p.round || 1}: passed`
          : `Three-stage review round ${p.round || 1}: not passed (${(p.failed || []).join(", ")})`);
        break;
      case "review.finished":
        if (p.status === "degraded") addMessage("system", `Three-stage review degraded: ${p.reason || "cannot complete"}; returning the last version`);
        else if (p.status === "unapproved") addMessage("system", "Three-stage review not approved: revision limit reached; returning the last version (unapproved)");
        break;
      case "config.changed": addMessage("system", "Kernel configuration updated"); break;
      case "permission.denied": addMessage("system", "Denied: " + (p.reason || "")); break;
      case "plugin.degraded": addMessage("system", "Plugin degraded (3 consecutive failures)"); break;
      case "subscriber.lagged": addMessage("system", "Subscription lagged; old events dropped"); break;
      case "memory.recalled": if ((p.items || []).length) addMessage("system", `Recalled ${p.items.length} memories`); break;
      case "chat.finished":
        addMessage("system", `— Turn complete (${p.status || "done"})`);
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
      ? `Choose a discovered model (${cfgModels.length})`
      : "No models found (manual entry allowed)";
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
    $("#cfg-models-hint").textContent = "Reading model list…";
    try {
      const v = await rpc("provider.models", { provider, base_url: baseUrl, model, api_key: apiKey }, 45000);
      cfgModels = [...new Set(Array.isArray(v.models) ? v.models : [])].sort();
      renderModelList();
      if (v.error) {
        $("#cfg-models-hint").textContent = "Cannot read model list (manual model name allowed)";
        setSettingsStatus("Model list: " + v.error, "warn");
      } else {
        $("#cfg-models-hint").textContent = `${cfgModels.length} models; type directly or choose from the list`;
      }
    } catch (e) {
      cfgModels = [];
      renderModelList();
      $("#cfg-models-hint").textContent = "Cannot read model list (manual model name allowed)";
      setSettingsStatus("Model list failed: " + e.message, "err");
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
      d.textContent = "No registered plugins yet";
      nav.appendChild(d);
      return;
    }
    for (const r of regs) {
      const b = document.createElement("button");
      b.className = "nav-item plugin-nav-item";
      b.textContent = `${r.enabled === false ? "○" : "●"} ${r.name}`;
      b.title = `${r.name} · ${r.category || "plugin"} · ${r.enabled === false ? "Disabled" : "Enabled"}`;
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
      box.innerHTML = '<div class="plugin-empty">No registered plugins.</div>';
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
        <div class="plugin-meta">${enabled ? (live.state === "ready" ? "Running" : "Start on demand") : "Disabled"} · ${esc((r.blake3 || "").slice(0, 12))}</div>
        <code>${esc(r.program || "")}</code>`;
      const acts = document.createElement("div");
      acts.className = "plugin-actions";
      const detail = document.createElement("button");
      detail.textContent = "Details";
      detail.onclick = () => showPluginDetail(r.name);
      const toggle = document.createElement("button");
      toggle.textContent = enabled ? "Disable" : "Enable";
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
    setPluginStatus(`${enabled ? "Disabling" : "Enabling"} ${name}…`, "warn");
    try {
      await rpc(enabled ? "plugin.disable" : "plugin.enable", { name });
      setPluginStatus(`${name} ${enabled ? "disabled" : "enabled"} (running process synced)`, "ok");
      await refreshPlugins();
      if (pluginDetailName === name) showPluginDetail(name);
    } catch (e) {
      setPluginStatus(`Operation failed: ${e.message}`, "err");
    }
  }

  async function installPlugin() {
    const path = ($("#plugin-install-path").value || "").trim();
    if (!path) {
      setPluginStatus("Enter the absolute path to the plugin directory", "err");
      return;
    }
    setPluginStatus("Installing…", "warn");
    try {
      const v = await rpc("plugin.install", { path }, 30000);
      const name = (v.installed && v.installed.name) || path;
      setPluginStatus(`Installed ${name}`, "ok");
      $("#plugin-install-path").value = "";
      await refreshPlugins();
    } catch (e) {
      setPluginStatus(`Install failed: ${e.message}`, "err");
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
    const titles = { general: "Settings", plugins: "Plugin management", plugin: "Plugin details" };
    $("#settings-title").textContent = titles[name] || "Settings";
    const subtitles = {
      general: "Saved and submitted to the kernel via config.set; workspace changes apply after the next serve restart.",
      plugins: "Plugins come from the kernel registry; installing registers the manifest, and toggling syncs the running process.",
      plugin: "Settings are declared by plugins in plugin.json and relayed verbatim by the kernel; installing new plugins requires no frontend changes.",
    };
    $("#settings-subtitle").textContent = subtitles[name] || subtitles.general;
    if (name !== "plugin") pluginDetailName = "";
  }

  // 按点号路径把值写进嵌套对象：`guide.model` → {guide:{model:...}}
  function setPath(obj, path, value) {
    const parts = String(path).split(".");
    let cur = obj;
    for (let i = 0; i < parts.length - 1; i++) {
      if (!cur[parts[i]] || typeof cur[parts[i]] !== "object") cur[parts[i]] = {};
      cur = cur[parts[i]];
    }
    cur[parts[parts.length - 1]] = value;
    return obj;
  }

  function fieldInputId(f) {
    return "ps-" + String(f.key || "").replace(/[^a-zA-Z0-9_-]/g, "_");
  }

  // 渲染一个声明字段；TUI/WebUI 都只认 bool/int/number/string/enum 五种类型。
  function pluginFieldHtml(f, value, models) {
    const id = fieldInputId(f);
    const label = esc(f.label || f.key || "?");
    const desc = f.description ? `<span class="form-hint">${esc(f.description)}</span>` : "";
    const type = f.type || "string";
    if (type === "bool") {
      return `<label class="check-row"><input id="${id}" type="checkbox" ${value ? "checked" : ""}> ${label}</label>${desc}`;
    }
    if (type === "enum") {
      const opts = (f.options || []).map((o) =>
        `<option value="${esc(o)}" ${o === value ? "selected" : ""}>${esc(o)}</option>`).join("");
      return `<label>${label}<select id="${id}">${opts}</select></label>${desc}`;
    }
    const attrs = [
      f.min !== undefined && f.min !== null ? `min="${f.min}"` : "",
      f.max !== undefined && f.max !== null ? `max="${f.max}"` : "",
      f.step !== undefined && f.step !== null ? `step="${f.step}"` : "",
    ].join(" ");
    if (type === "int" || type === "number") {
      return `<label>${label}<input id="${id}" type="number" ${attrs} value="${esc(value ?? f.default ?? "")}"></label>${desc}`;
    }
    // options_source == "models"：下拉来自本地模型服务，同时保留手动输入（模型名可能是服务没报的）。
    if (f.options_source === "models") {
      const list = Array.isArray(models) ? models : [];
      const current = value === null || value === undefined ? "" : String(value);
      let opts = `<option value=""${current === "" ? " selected" : ""}>（Follow current model）</option>`;
      for (const m of list) {
        opts += `<option value="${esc(m)}"${m === current ? " selected" : ""}>${esc(m)}</option>`;
      }
      if (current && !list.includes(current)) {
        opts += `<option value="${esc(current)}" selected>${esc(current)} (Custom)</option>`;
      }
      const hint = list.length
        ? `<span class="form-hint">Local model service returned ${list.length} models; choosing from the dropdown fills the input below, or type directly</span>`
        : `<span class="form-hint">Model list unavailable; type the model name directly</span>`;
      return `<label>${label}<select id="${id}-pick">${opts}</select>` +
        `<input id="${id}" type="text" placeholder="${esc(f.placeholder || "")}" value="${esc(current)}"></label>${desc}${hint}`;
    }
    const kind = f.secret ? "password" : "text";
    return `<label>${label}<input id="${id}" type="${kind}" placeholder="${esc(f.placeholder || "")}" value="${esc(value ?? "")}"></label>${desc}`;
  }

  /// 下拉选中后写进同名输入框：保存逻辑只读输入框，避免两处取值打架。
  function wirePluginPickers(fields) {
    for (const f of fields) {
      if (f.options_source !== "models") continue;
      const pick = document.getElementById(`${fieldInputId(f)}-pick`);
      const input = document.getElementById(fieldInputId(f));
      if (!pick || !input) continue;
      pick.onchange = () => { input.value = pick.value; };
    }
  }

  function readFieldInput(f) {
    const el = document.getElementById(fieldInputId(f));
    if (!el) return { value: f.default };
    if (f.type === "bool") return { value: el.checked };
    if (f.type === "int") {
      const n = Number(el.value);
      if (!Number.isInteger(n)) throw new Error(`${f.label} must be an integer`);
      return { value: n };
    }
    if (f.type === "number") {
      const n = Number(el.value);
      if (!Number.isFinite(n)) throw new Error(`${f.label} must be a number`);
      return { value: n };
    }
    return { value: el.value };
  }

  async function savePluginSettings(name) {
    if (!pluginSettings || pluginSettings.name !== name) return;
    const patch = {};
    for (const f of pluginSettings.fields) {
      const path = f.path && String(f.path).trim() ? f.path.trim()
        : `${pluginSettings.root}.${f.key}`;
      setPath(patch, path, readFieldInput(f).value);
    }
    try {
      await rpc("config.set", { patch }, 15000);
      const st = document.getElementById("plugin-settings-status");
      if (st) { st.textContent = `Saved settings for ${name}`; st.className = "form-hint ok"; }
      cfg = await rpc("config.get", {}, 15000);
    } catch (e) {
      const st = document.getElementById("plugin-settings-status");
      if (st) { st.textContent = "Save failed: " + e.message; st.className = "form-hint err"; }
    }
  }

  async function showPluginDetail(name) {
    const r = pluginRegistry(name);
    if (!r) return;
    const live = pluginLive(name);
    const m = live.manifest || {};
    pluginDetailName = name;
    showSettingsView("plugin");
    $("#settings-title").textContent = `Plugin · ${name}`;
    $("#plugin-nav")?.querySelectorAll(".plugin-nav-item").forEach((b) => {
      b.classList.toggle("active", b.textContent.includes(name));
    });
    const enabled = r.enabled !== false;
    const caps = Array.isArray(m.capabilities) ? m.capabilities : [];
    const perms = Array.isArray(m.permissions) ? m.permissions : [];
    const head = `
      <div class="settings-section">
        <div class="section-title">Plugin Manifest</div>
        <div class="plugin-detail-head">
          <div><h3>${esc(name)}</h3><p class="warn">${esc(name)} · ${esc(r.category || "plugin")} · version ${esc(m.version || "Not running/not read")} · ${enabled ? "Enabled" : "Disabled"}</p></div>
          <button id="btn-plugin-detail-toggle" class="${enabled ? "ghost" : "primary"}">${enabled ? "Disable" : "Enable"}</button>
        </div>
        <div class="kv-grid">
          <div><span>API</span><code>${esc(m.api || "-")}</code></div>
          <div><span>live</span><code>${esc(live.state || "stopped")}</code></div>
          <div><span>capabilities</span><code>${esc(caps.join(", ") || "-")}</code></div>
          <div><span>permissions</span><code>${esc(perms.join(", ") || "-")}</code></div>
          <div class="wide"><span>program</span><code>${esc(r.program || "-")}</code></div>
          <div class="wide"><span>blake3</span><code>${esc(r.blake3 || "-")}</code></div>
        </div>
      </div>`;

    // 插件声明了Settings项 → 渲染它自己的表单；没声明就只有清单页。
    let settings = null;
    try {
      settings = await rpc("plugin.settings", { name }, 15000);
    } catch (e) {
      settings = null;
    }
    pluginSettings = settings;
    let form = "";
    if (settings && Array.isArray(settings.fields) && settings.fields.length) {
      // 模型类字段的候选来自本地模型服务；前端统一取一次，所有字段共用。
      let models = [];
      if (settings.fields.some((f) => f.options_source === "models")) {
        try {
          const mv = await rpc("provider.models", {}, 45000);
          models = Array.isArray(mv.models) ? mv.models : [];
        } catch (e) {
          models = [];
        }
      }
      const rows = settings.fields.map((f) => {
        const v = (settings.values || {})[f.key];
        return pluginFieldHtml(f, v === null || v === undefined ? f.default : v, models);
      }).join("");
      form = `
      <div class="settings-section">
        <div class="section-title">${esc(settings.title || "Plugin settings")}</div>
        ${settings.description ? `<p class="warn">${esc(settings.description)}</p>` : ""}
        <div class="settings-grid">${rows}</div>
        <div class="settings-actions">
          <span id="plugin-settings-status" class="hint"></span>
          <button id="btn-plugin-settings-save" class="primary">Save plugin settings</button>
        </div>
      </div>`;
    }
    $("#plugin-detail").innerHTML = head + form;
    if (settings && Array.isArray(settings.fields)) wirePluginPickers(settings.fields);
    $("#btn-plugin-detail-toggle").onclick = () => togglePlugin(name, enabled);
    const save = document.getElementById("btn-plugin-settings-save");
    if (save) save.onclick = () => savePluginSettings(name);
  }

  async function openSettings() {
    setSettingsStatus("Reading configuration…", "warn");
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
      syncTemperature();
      try {
        await refreshPlugins();
      } catch (e) {
        setPluginStatus("Failed to read plugins: " + e.message, "err");
      }
      showSettingsView("general");
      setSettingsStatus("");
      $("#btn-settings-save").disabled = false;
      $("#btn-settings-refresh").disabled = false;
      await refreshModels();
    } catch (e) {
      $("#btn-settings-save").disabled = false;
      $("#btn-settings-refresh").disabled = false;
      setSettingsStatus("Failed to read configuration: " + e.message, "err");
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
      setSettingsStatus("Model name and Base URL must not be empty", "err");
      return;
    }
    if (!Number.isFinite(temperature) || temperature < 0 || temperature > 2) {
      setSettingsStatus("Temperature must be between 0.00 and 2.00", "err");
      return;
    }
    setSettingsStatus("Submitting to kernel…", "warn");
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
      }});
      cfg = v.config || cfg;
      setSettingsStatus("Saved and applied; the next inference uses the new configuration", "ok");
      addMessage("system", `Configuration applied: ${cfg.provider_plugin} / ${cfg.model} / T=${cfg.temperature}`);
      // 上下文面板在下一轮 chat.finished 后刷新；此处不阻塞Settings保存。
    } catch (e) {
      setSettingsStatus("Save failed: " + e.message, "err");
    }
  }

  function openWs() {
    return new Promise((resolve, reject) => {
      let settled = false;
      ws = new WebSocket(`ws://${location.host}/ws?token=${encodeURIComponent(token)}`);
      ws.onopen = () => { settled = true; resolve(); };
      ws.onerror = () => { if (!settled) reject(new Error("WebSocket connection failed")); };
      ws.onclose = () => {
        $("#conn").textContent = "WS disconnected (refresh to reconnect)";
        for (const [, p] of pending) {
          clearTimeout(p.timer);
          p.reject(new Error("WS disconnected"));
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
          if (v.error) p.reject(new Error(v.error.message || "RPC error"));
          else p.resolve(v.result);
          return;
        }
        handleNotification(v);
      };
      setTimeout(() => { if (!settled) reject(new Error("WS handshake timed out")); }, 3000);
    });
  }

  function openSse() {
    const es = new EventSource("/events?token=" + encodeURIComponent(token));
    es.onopen = () => { $("#conn").textContent = "Kernel connected (HTTP/SSE fallback)"; };
    es.onerror = () => { $("#conn").textContent = "SSE reconnecting…"; };
    es.onmessage = (ev) => {
      let v;
      try { v = JSON.parse(ev.data); } catch { return; }
      handleNotification(v);
    };
  }

  function bindUi() {
    $("#btn-send").onclick = send;
    $("#btn-attach").onclick = () => $("#file-input").click();
    $("#file-input").onchange = async (e) => {
      const files = Array.from(e.target.files || []);
      e.target.value = "";
      await attachFiles(files);
    };
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
    $("#btn-plugin-refresh").onclick = () => refreshPlugins().then(() => setPluginStatus("Plugin list refreshed", "ok")).catch((e) => setPluginStatus("Refresh failed: " + e.message, "err"));
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
      $("#conn").textContent = "Kernel connected (WebSocket)";
    } catch {
      openSse();
      $("#conn").textContent = "WebSocket unavailable; falling back to HTTP/SSE";
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
      $("#conn").textContent = "Initialization failed: " + e.message;
    }
  })();
})();
