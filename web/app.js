// FeatherAgent WebUI：默认 WebSocket（同连接 RPC + 事件推送），失败回退 HTTP + SSE。
// token 从 URL fragment 读取后立刻从地址栏抹掉（不进 Referer、不进历史标题）。

(() => {
  const $ = (s) => document.querySelector(s);

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
    $("#state-label").textContent = state;
    const dot = $("#state-dot");
    dot.className = "dot " + (state === "running" || state === "generating" ? "running" : state === "cancelled" ? "cancelled" : "idle");
    $("#session-label").textContent = current ? "会话 " + current : "未选择会话";
  }

  function addMessage(role, text) {
    if (!text) return;
    const div = document.createElement("div");
    div.className = "msg " + role;
    div.innerHTML = `<span class="role">${esc(role)}</span>${esc(text)}`;
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
      div.innerHTML = `<div class="sid">${esc(s.id)}</div><div class="smeta">${s.records || 0} 条 · ${esc(live)}</div>`;
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

  async function resume(id) {
    try {
      await rpc("session.resume", { session: id });
      current = id;
      $("#messages").innerHTML = "";
      addMessage("system", "已恢复会话 " + id);
      $("#hint").textContent = "";
      renderSessions();
      await refreshCtx();
    } catch (e) {
      $("#hint").textContent = "恢复失败：" + e.message;
    }
  }

  async function createSession() {
    const id = ($("#new-id").value || "").trim() || "s-webui-" + Date.now().toString(36);
    try {
      await rpc("session.create", { id });
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
    addMessage("user", text);
    try {
      await rpc("chat.send", { session: current, text });
      setState("running");
      $("#hint").textContent = "";
    } catch (e) {
      addMessage("system", "发送失败：" + e.message);
      $("#hint").textContent = e.message;
    }
  }

  async function cancel() {
    if (!current) return;
    try {
      await rpc("chat.cancel", { session: current });
      $("#hint").textContent = "已请求取消";
    } catch (e) {
      $("#hint").textContent = "取消失败：" + e.message;
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
      case "config.changed": addMessage("system", "内核配置已更新"); break;
      case "permission.denied": addMessage("system", "拒绝：" + (p.reason || "")); break;
      case "plugin.degraded": addMessage("system", "插件降级（连续失败 3 次）"); break;
      case "subscriber.lagged": addMessage("system", "订阅落后，已丢弃旧事件"); break;
      case "memory.recalled": if ((p.items || []).length) addMessage("system", `召回 ${p.items.length} 条记忆`); break;
      case "chat.finished":
        addMessage("system", `— 本轮结束（${p.status || "done"}）`);
        setState(p.status === "cancelled" ? "cancelled" : "idle");
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
    $("#cfg-models-hint").textContent = "正在读取模型列表…";
    try {
      const v = await rpc("provider.models", { provider, base_url: baseUrl, model }, 45000);
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
      $("#cfg-model").value = cfg.model || "qwen2.5:7b";
      syncModelSelect();
      $("#cfg-temperature").value = String(cfg.temperature ?? 0.7);
      $("#cfg-context-window").value = String(cfg.context_window ?? 8192);
      $("#cfg-max-turns").value = String(cfg.max_turns ?? 6);
      $("#cfg-auto-allow-safe").checked = cfg.auto_allow_safe !== false;
      syncTemperature();
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
    setSettingsStatus("正在提交给内核…", "warn");
    try {
      const v = await rpc("config.set", { patch: {
        provider_plugin: $("#cfg-provider").value,
        base_url: baseUrl,
        model,
        temperature,
        context_window: contextWindow,
        max_turns: maxTurns,
        auto_allow_safe: $("#cfg-auto-allow-safe").checked,
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
    $("#btn-cancel").onclick = cancel;
    $("#btn-fork").onclick = fork;
    $("#btn-new").onclick = createSession;
    $("#btn-settings").onclick = openSettings;
    $("#btn-settings-close").onclick = closeSettings;
    $("#btn-settings-cancel").onclick = closeSettings;
    $("#btn-settings-save").onclick = saveSettings;
    $("#btn-settings-refresh").onclick = refreshModels;
    $("#cfg-provider").onchange = refreshModels;
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
    window.addEventListener("keydown", (e) => { if (e.key === "Escape") closeSettings(); });
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
      await refreshSessions();
      if (sessions.length) await resume(sessions[0].id);
      else await createSession();
    } catch (e) {
      $("#conn").textContent = "初始化失败：" + e.message;
    }
  })();
})();
