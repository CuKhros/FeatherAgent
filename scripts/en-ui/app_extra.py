EXTRA = [
"""
      <div class="settings-section">
        <div class="section-title">${esc(settings.title || "Plugin settings")}</div>
        ${settings.description ? """,
"""
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
      </div>""",
""" : ""}
        <div class="settings-grid">${rows}</div>
        <div class="settings-actions">
          <span id="plugin-settings-status" class="hint"></span>
          <button id="btn-plugin-settings-save" class="primary">Save plugin settings</button>
        </div>
      </div>""",
"${cfgModels.length} models; type directly or choose from the list",
'${enabled ? "Disabling" : "Enabling"} ${name}…',
"${f.label} must be a number",
"${f.label} must be an integer",
'${name} ${enabled ? "disabled" : "enabled"} (running process synced)',
"${params.tool} (risk: ${params.risk})",
'${r.name} · ${r.category || "plugin"} · ${r.enabled === false ? "Disabled" : "Enabled"}',
'${w.name}${w.has_agent ? " · project" : ""}',
'<div class="sid">${esc(s.id)}</div><div class="smeta">${esc(s.workspace || "default")} · ${s.records || 0} events · ${esc(live)}</div>',
"""<div><strong>${esc(r.name)}</strong> <span class="tag">${esc(r.category || "plugin")}</span></div>
        <div class="plugin-meta">${enabled ? (live.state === "ready" ? "Running" : "Start on demand") : "Disabled"} · ${esc((r.blake3 || "").slice(0, 12))}</div>
        <code>${esc(r.program || "")}</code>""",
'<option value=""${current === "" ? " selected" : ""}>（Follow current model）</option>',
'<option value="${esc(current)}" selected>${esc(current)} (Custom)</option>',
'<span class="form-hint">Local model service returned ${list.length} models; choosing from the dropdown fills the input below, or type directly</span>',
'<span class="form-hint">Model list unavailable; type the model name directly</span>',
'<span class="k">L1</span> ${layers.l1_window_messages || 0} items · <span class="k">L2</span> ${layers.l2_summary_cards || 0} cards · <span class="k">L3</span> ${layers.l3_task_state_tokens || 0} tok · <span class="k">L4</span> ${layers.l4_recalled_items || 0} items',
'<span class="k">Model</span> ${esc(snap.model || "-")} · <span class="k">Events</span> ${snap.turns || 0} · <span class="k">Turns</span> ${v.turn || 0}',
'<span class="k">Goal</span> ${esc(goal)}',
'<span class="k">Budget</span> ${total} / ${world} tokens (${pct}%)<div class="bar"><i style="width:${pct}%"></i></div>',
"RPC timeout: ${method}",
'Three-stage review round ${p.round || 1}: not passed (${(p.failed || []).join(", ")})',
'Three-stage review round ${p.round || 1}: passed',
'Three-stage review degraded: ${p.reason || "cannot complete"}; returning the last version',
'Three-stage review: generated ${p.count || 0} acceptance items',
'Context compacted: ${n} old events → ${p.after_chars || 0}-char summary (original text remains in the log)',
'Recalled ${p.items.length} memories',
"Install failed: ${e.message}",
'Tool failed: ${p.error || p.name || ""}',
'Tool circuit-breaker: ${p.name || "?"} (${p.failures || 3} consecutive technical failures)',
'Forked, copied ${v.copied} events',
'Saved settings for ${name}',
'Created project workspace ${name} with its own .featheagent/',
'Context compacted: ${v.compacted_events} old events → ${v.summary_chars}-char summary',
'Installed ${name}',
'Restored session ${id} | project ${ws} | ${v.total || 0} events',
'Plugin · ${name}',
"Operation failed: ${e.message}",
'Choose a discovered model (${cfgModels.length})',
'Configuration applied: ${cfg.provider_plugin} / ${cfg.model} / T=${cfg.temperature}',
]
