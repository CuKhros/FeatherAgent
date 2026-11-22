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

  function wsRpc(method, params = {}) {
    return new Promise((resolve, reject) => {
      const id = rpcId++;
      pending.set(id, { resolve, reject });
      try {
        ws.send(JSON.stringify({ jsonrpc: "2.0", id, method, params }));
      } catch (e) {
        pending.delete(id);
        reject(e);
      }
    });
  }

  async function rpc(method, params = {}) {
    if (ws && ws.readyState === WebSocket.OPEN) return wsRpc(method, params);
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
      const world = 8192;
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
    if (p.session && p.session !== current) return;
    switch (k) {
      case "model.message": if (p.text) addMessage("assistant", p.text); break;
      case "tool.requested": addMessage("tool", `请求 ${p.tool || "?"}`); break;
      case "tool.result": {
        let brief = "";
        try { brief = JSON.stringify(p.result); } catch { brief = String(p.result); }
        addMessage("tool", `${p.name || "?"} → ${brief.slice(0, 400)}`);
        break;
      }
      case "tool.failed": addMessage("tool", `${p.name || "?"} 失败: ${p.error || ""}`); break;
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

  function openWs() {
    return new Promise((resolve, reject) => {
      let settled = false;
      ws = new WebSocket(`ws://${location.host}/ws?token=${encodeURIComponent(token)}`);
      ws.onopen = () => { settled = true; resolve(); };
      ws.onerror = () => { if (!settled) reject(new Error("WS 连接失败")); };
      ws.onclose = () => {
        $("#conn").textContent = "WS 已断开（刷新页面重连）";
        ws = null;
      };
      ws.onmessage = (ev) => {
        let v;
        try { v = JSON.parse(ev.data); } catch { return; }
        if (v.id !== undefined && pending.has(v.id)) {
          const p = pending.get(v.id);
          pending.delete(v.id);
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
    $("#btn-allow").onclick = () => answerConfirm(true);
    $("#btn-deny").onclick = () => answerConfirm(false);
    $("#input").addEventListener("keydown", (e) => {
      if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); send(); }
    });
    $("#new-id").addEventListener("keydown", (e) => { if (e.key === "Enter") createSession(); });
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
      await rpc("event.subscribe", { kinds: [] });
      await refreshSessions();
      if (sessions.length) await resume(sessions[0].id);
      else await createSession();
    } catch (e) {
      $("#conn").textContent = "初始化失败：" + e.message;
    }
  })();
})();
