// SPDX-License-Identifier: GPL-3.0-or-later

//! The WebUI JavaScript (extracted from `html.rs` in v0.4 B1).
//! Plain JS text; the document in `html.rs` splices it into the
//! `<script>` element.

pub const WEB_INDEX_JS: &str = r#"    let activeTab = 'chat';

    // Web sessions (v0.4 B1): one active chat at a time. The list
    // comes from GET /api/sessions; switching replays the chat's
    // transcript from disk and resubscribes SSE to that session, so
    // a chat survives a restart and a second tab never sees the
    // first tab's events.
    let activeSession = { id: null };
    let eventSource = { current: null };
    // Switch generation (v0.4 B3 fix P2-2): each switchSession call
    // takes a generation; a fetch that resolves after a newer
    // switch started is stale and must not render its transcript
    // or open its SSE stream (rapid double-click otherwise shows
    // session B as active while streaming session A's events).
    let switchGen = 0;

    // Tool-call inspector + live Clowder panel (v0.4 B3): every
    // ToolCallStarted/Finished/ApprovalRequested is stored by
    // call_id so a click on a tool card renders its full detail
    // (arguments, result, diff preview, approve/reject) in the
    // inspector column; SubagentSpawned/StatusChanged render the
    // live subagent list in the sidebar. Model-controlled values
    // are rendered through escapeHtml/textContent — never raw.
    const toolCalls = new Map();
    let selectedCallId = null;
    const subagents = new Map();

    const WELCOME_HTML = `<div class="msg assistant"><div class="msg-header">🐾 PurrCode AI (Meowstro)</div><div>Selamat datang di PurrCode AI WebUI! Sistem orkestrasi The Clowder dan Gateway aktif. Ada yang bisa saya bantu analisa atau kerjakan?</div></div>`;

    async function loadSessions() {
      const listEl = document.getElementById('session-list');
      try {
        const res = await fetch(apiUrl('/api/sessions'));
        const sessions = await res.json();
        renderSessionList(sessions);
        if (sessions.length === 0) {
          await createSession();
        } else {
          await switchSession(sessions[0].id);
        }
      } catch (e) {
        if (listEl) listEl.innerHTML = `<div style="color: var(--danger); font-size: 0.85rem;">Failed to load sessions: ${escapeHtml(String(e))}</div>`;
      }
    }

    function renderSessionList(sessions) {
      const listEl = document.getElementById('session-list');
      if (!listEl) return;
      listEl.innerHTML = '';
      for (const s of sessions) {
        const btn = document.createElement('button');
        btn.className = 'session-item' + (s.id === activeSession.id ? ' active' : '');
        btn.dataset.sessionId = s.id;
        const title = document.createElement('span');
        title.className = 'session-title';
        title.textContent = s.title || s.id;
        const meta = document.createElement('span');
        meta.className = 'session-meta';
        meta.textContent = s.id + ' · ' + s.message_count + ' pesan';
        btn.append(title, meta);
        btn.addEventListener('click', () => switchSession(s.id));
        listEl.appendChild(btn);
      }
      if (sessions.length === 0) {
        listEl.innerHTML = '<div style="color: var(--text-muted); font-size: 0.85rem;">Belum ada sesi.</div>';
      }
    }

    async function createSession() {
      try {
        const res = await fetch(apiUrl('/api/sessions'), { method: 'POST' });
        const data = await res.json();
        if (!data.id) return;
        const listRes = await fetch(apiUrl('/api/sessions'));
        renderSessionList(await listRes.json());
        await switchSession(data.id);
      } catch (e) {
        console.error('Failed to create session:', e);
      }
    }

    async function switchSession(id) {
      if (activeSession.id === id) return;
      const gen = ++switchGen;
      // Close the old stream FIRST, synchronously: setupSSE only
      // runs after the awaited transcript fetch below, and until
      // then the old session's events would keep arriving into the
      // new session's maps and the cleared messages container.
      if (eventSource.current) eventSource.current.close();
      eventSource.current = null;
      activeSession.id = id;
      // Live state belongs to the previous session: tool-call
      // detail and the Clowder list come from that session's SSE
      // feed only, so switching resets them (a resumed session's
      // subagents are history, not live).
      toolCalls.clear();
      subagents.clear();
      selectedCallId = null;
      const insp = document.getElementById('inspector-content');
      if (insp) insp.innerHTML = '<div style="color: var(--text-muted); font-size: 0.85rem;">Klik tool call untuk melihat detail.</div>';
      renderClowder();
      document.querySelectorAll('.session-item').forEach(b => b.classList.toggle('active', b.dataset.sessionId === id));
      const container = document.getElementById('messages');
      container.innerHTML = '';
      try {
        const res = await fetch(apiUrl('/api/sessions/' + encodeURIComponent(id) + '/transcript'));
        if (gen !== switchGen) return;
        if (res.ok) {
          const data = await res.json();
          if (gen !== switchGen) return;
          renderTranscript(data.messages || []);
        }
      } catch (e) {
        console.error('Failed to load transcript:', e);
      }
      if (gen !== switchGen) return;
      if (!container.children.length) container.innerHTML = WELCOME_HTML;
      setupSSE(id);
    }

    function renderTranscript(messages) {
      const container = document.getElementById('messages');
      for (const m of messages) {
        if (m.role === 'system') continue;
        const div = document.createElement('div');
        if (m.role === 'user') {
          div.className = 'msg user';
          div.innerHTML = `<div class="msg-header">You</div><div>${escapeHtml(m.content || '')}</div>`;
        } else if (m.role === 'tool') {
          div.className = 'msg tool';
          const snippet = (m.content || '').slice(0, 300);
          div.textContent = '🔧 tool result: ' + snippet;
        } else {
          div.className = 'msg assistant';
          const calls = (m.tool_calls || []).map(c => '🔧 ' + (c.name || '')).join(' ');
          div.innerHTML = `<div class="msg-header">🐾 PurrCode AI</div><div class="bot-content" style="white-space: pre-wrap;">${escapeHtml(m.content || '')}</div>` + (calls ? `<div style="color: var(--text-muted); font-size: 0.85rem;">${escapeHtml(calls)}</div>` : '');
        }
        container.appendChild(div);
      }
      container.scrollTop = container.scrollHeight;
    }

    // When the server was started with a token, the page is opened as
    // `/?token=...` and every API call carries it back (otherwise the
    // token-protected server answers 401 to the UI's own requests).
    const urlToken = new URLSearchParams(location.search).get('token');
    function apiUrl(path) {
      if (!urlToken) return path;
      return path + (path.includes('?') ? '&' : '?') + 'token=' + encodeURIComponent(urlToken);
    }

    function switchTab(tab) {
      document.querySelectorAll('.nav-btn').forEach(b => b.classList.remove('active'));
      document.querySelectorAll('.content').forEach(c => c.style.display = 'none');

      const btn = Array.from(document.querySelectorAll('.nav-btn')).find(b => b.getAttribute('onclick').includes(tab));
      if (btn) btn.classList.add('active');

      const content = document.getElementById(`tab-${tab}`);
      if (content) {
        content.style.display = tab === 'chat' ? 'flex' : 'block';
      }

      const titles = {
        chat: 'Terminal & Chat',
        cron: 'Autonomous Cron Scheduler',
        skills: 'Skills & Curator Engine',
        memory: 'Identity & Memory Store',
        plugins: 'Plugins & Sandboxing'
      };
      document.getElementById('tab-title').textContent = titles[tab] || 'PurrCode AI';
      activeTab = tab;

      if (tab === 'cron') loadCronJobs();
      if (tab === 'skills') loadCandidates();
      if (tab === 'plugins') loadPlugins();
    }

    async function loadCronJobs() {
      try {
        const res = await fetch(apiUrl('/api/cron'));
        if (!res.ok) return;
        const jobs = await res.json();
        const tbody = document.getElementById('cron-tbody');
        if (!jobs || jobs.length === 0) {
          tbody.innerHTML = '<tr><td colspan="6" style="text-align: center; color: var(--text-muted);">No scheduled cron jobs found.</td></tr>';
          return;
        }
        tbody.innerHTML = jobs.map(j => `
          <tr>
            <td><code>${escapeHtml(j.id || '')}</code></td>
            <td><strong>${escapeHtml(j.schedule || '')}</strong></td>
            <td>${escapeHtml(j.prompt || '')}</td>
            <td><code>${escapeHtml(j.allowed_tools ? j.allowed_tools.join(', ') : 'read-only (default)')}</code></td>
            <td>${escapeHtml(j.next_run || 'pending')}</td>
            <td><span class="tag ${j.enabled ? 'primary' : ''}">${j.enabled ? 'Enabled' : 'Disabled'}</span></td>
          </tr>
        `).join('');
      } catch (e) {
        console.error('Failed to load cron jobs:', e);
      }
    }

    async function loadCandidates() {
      try {
        const res = await fetch(apiUrl('/api/skills'));
        if (!res.ok) return;
        const data = await res.json();
        const container = document.getElementById('candidates-container');
        const candidates = data.candidates || [];
        if (candidates.length === 0) {
          container.innerHTML = '<div style="color: var(--text-muted);">No pending candidate skills found.</div>';
          return;
        }
        container.innerHTML = candidates.map(c => `
          <div class="view-card" style="display: flex; justify-content: space-between; align-items: center;">
            <div>
              <strong>${escapeHtml(c.name || '')}</strong>
              <div style="color: var(--text-muted); font-size: 0.85rem;">${escapeHtml(c.description || '')}</div>
            </div>
            <button class="btn btn-primary" data-skill-name="${escapeHtml(c.name || '')}">Approve Skill</button>
          </div>
        `).join('');
        // R3 fix (P1-3): no inline onclick — the skill name is
        // model-controlled; it travels in a data attribute (decoded
        // by the parser) and is read back via dataset, never
        // interpolated into a JS string literal.
        container.querySelectorAll('button[data-skill-name]').forEach(btn => {
          btn.addEventListener('click', () => approveSkill(btn.dataset.skillName));
        });
      } catch (e) {
        console.error('Failed to load skills:', e);
      }
    }

    async function approveSkill(name) {
      try {
        const res = await fetch(apiUrl('/api/skills/approve'), {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ name })
        });
        const data = await res.json().catch(() => ({}));
        if (data.resolved) {
          alert(`🐾 Skill '${name}' approved successfully!`);
          loadCandidates();
        } else {
          alert(`No pending approval for '${name}' — the draft card may have expired or was already resolved.`);
          loadCandidates();
        }
      } catch (e) {
        alert('Failed to approve skill: ' + e);
      }
    }

    async function loadMemoryDoc(doc) {
      try {
        const res = await fetch(apiUrl(`/api/memory?file=${doc}`));
        if (!res.ok) return;
        const text = await res.text();
        document.getElementById('memory-content').textContent = text;
      } catch (e) {
        console.error('Failed to load memory doc:', e);
      }
    }

    async function loadPlugins() {
      try {
        const res = await fetch(apiUrl('/api/plugins'));
        if (!res.ok) return;
        const data = await res.json();
        document.getElementById('plugins-content').textContent = JSON.stringify(data, null, 2);
      } catch (e) {
        console.error('Failed to load plugins:', e);
      }
    }

    async function submitPrompt() {
      const input = document.getElementById('prompt-input');
      const text = input.value.trim();
      if (!text) return;
      input.value = '';

      const container = document.getElementById('messages');
      const userMsg = document.createElement('div');
      userMsg.className = 'msg user';
      userMsg.innerHTML = `<div class="msg-header">You</div><div>${escapeHtml(text)}</div>`;
      container.appendChild(userMsg);

      const botMsg = document.createElement('div');
      botMsg.className = 'msg assistant';
      botMsg.innerHTML = `<div class="msg-header">🐾 PurrCode AI</div><div class="bot-content" style="white-space: pre-wrap;">Thinking...</div>`;
      container.appendChild(botMsg);
      container.scrollTop = container.scrollHeight;

      try {
        await fetch(apiUrl('/api/prompt'), {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ prompt: text, session_id: activeSession.id })
        });
      } catch (e) {
        botMsg.querySelector('.bot-content').textContent = 'Error: ' + e;
      }
    }

    function escapeHtml(str) {
      return str.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&#39;');
    }

    // Connect Server-Sent Events (SSE) for one session. Switching
    // chats closes the old stream first, so a tab only ever renders
    // its own session's events.
    function setupSSE(sessionId) {
      if (eventSource.current) eventSource.current.close();
      const es = new EventSource(apiUrl('/api/events?session=' + encodeURIComponent(sessionId)));
      eventSource.current = es;
      es.onmessage = function(event) {
        try {
          const data = JSON.parse(event.data);
          handleAgentEvent(data);
        } catch (e) {
          console.error('Error parsing SSE event:', e);
        }
      };
      es.onerror = function() {
        document.getElementById('gateway-status').textContent = 'Reconnecting...';
        document.querySelector('.status-dot').style.backgroundColor = 'var(--danger)';
      };
      es.onopen = function() {
        document.getElementById('gateway-status').textContent = 'Gateway Connected';
        document.querySelector('.status-dot').style.backgroundColor = 'var(--success)';
      };
    }

    // Events arrive internally tagged: {"type": "TextChunk",
    // "payload": {"text": "..."}} — the dev-claude reference used the
    // externally tagged shape ({"TextChunk": {...}}), which does not
    // match this repo's AgentEvent serde. The approval banner is new
    // in this port: the reference UI never rendered approval cards
    // (its CSS had the class, no JS used it), so a turn that needed
    // approval could never be resolved from the WebUI at all.
    function handleAgentEvent(ev) {
      const container = document.getElementById('messages');
      const lastBot = container.querySelector('.msg.assistant:last-child .bot-content');
      const kind = ev.type;
      const p = ev.payload || {};
      if (!kind) return;

      if (kind === 'TextChunk') {
        if (!lastBot) return;
        if (lastBot.textContent === 'Thinking...') lastBot.textContent = '';
        lastBot.textContent += p.text || '';
      } else if (kind === 'ReasoningChunk') {
        if (!lastBot) return;
        let rBox = lastBot.parentElement.querySelector('.reasoning-box');
        if (!rBox) {
          rBox = document.createElement('div');
          rBox.className = 'reasoning-box';
          lastBot.parentElement.insertBefore(rBox, lastBot);
        }
        rBox.textContent += p.thinking || '';
      } else if (kind === 'ToolCallStarted') {
        toolCalls.set(p.call_id || '', { tool: p.tool || '', arguments: p.arguments, status: 'running' });
        if (!lastBot) return;
        const card = document.createElement('div');
        card.className = 'tool-card';
        card.dataset.callId = p.call_id || '';
        card.innerHTML = `<div class="tool-header"><span>🔧 ${escapeHtml(p.tool || '')}</span><span>Executing...</span></div><pre>${escapeHtml(JSON.stringify(p.arguments, null, 2))}</pre>`;
        card.addEventListener('click', () => selectToolCall(p.call_id || ''));
        lastBot.parentElement.insertBefore(card, lastBot);
      } else if (kind === 'ToolCallFinished') {
        const entry = p.call_id ? toolCalls.get(p.call_id) : null;
        if (entry) {
          entry.result = p.result;
          entry.success = p.success;
          entry.status = p.success ? 'done' : 'failed';
        }
        const card = p.call_id ? container.querySelector(`.tool-card[data-call-id="${CSS.escape(p.call_id)}"]`) : null;
        if (card) {
          const status = card.querySelector('.tool-header span:last-child');
          if (status) status.textContent = p.success ? 'Done ✓' : 'Failed ✗';
        }
        if (selectedCallId === p.call_id) renderInspector();
      } else if (kind === 'ToolCallApprovalRequested') {
        const banner = document.createElement('div');
        banner.className = 'approval-banner';
        banner.dataset.callId = p.call_id || '';
        const preview = p.diff_preview ? `<pre style="margin-top:8px">${escapeHtml(p.diff_preview)}</pre>` : '';
        banner.innerHTML = `<div><strong>⚠️ Approval required — ${escapeHtml(p.tool || '')}</strong><div style="color:var(--text-muted);font-size:0.85rem">${escapeHtml(p.description || '')}</div>${preview}</div>`;
        // R3 fix (P1-3): no inline onclick — the call_id is
        // model-controlled (tool_calls[].id) and escapeHtml alone
        // cannot make it safe inside a JS string literal (a quote
        // breaks out). The id travels in a closure + dataset, never
        // interpolated into HTML.
        const actions = document.createElement('div');
        actions.className = 'approval-actions';
        const yes = document.createElement('button');
        yes.className = 'btn btn-primary';
        yes.textContent = 'Ya';
        yes.addEventListener('click', () => resolveApproval(p.call_id || '', true, yes));
        const no = document.createElement('button');
        no.className = 'btn btn-danger';
        no.textContent = 'Tidak';
        no.addEventListener('click', () => resolveApproval(p.call_id || '', false, no));
        actions.append(yes, no);
        banner.appendChild(actions);
        container.appendChild(banner);
        const entry = toolCalls.get(p.call_id || '');
        if (entry) {
          entry.description = p.description || '';
          entry.diffPreview = p.diff_preview || null;
          entry.status = 'pending-approval';
        } else {
          toolCalls.set(p.call_id || '', { tool: p.tool || '', arguments: null, description: p.description || '', diffPreview: p.diff_preview || null, status: 'pending-approval' });
        }
        if (selectedCallId === (p.call_id || '')) renderInspector();
      } else if (kind === 'SubagentSpawned') {
        if (p.subagent_id) {
          subagents.set(p.subagent_id, { role: p.role || 'subagent', brief: p.brief || '', status: 'pending', summary: null });
          renderClowder();
        }
      } else if (kind === 'SubagentStatusChanged') {
        if (p.subagent_id) {
          const s = subagents.get(p.subagent_id) || { role: 'subagent', brief: '', status: 'pending', summary: null };
          s.status = p.status || s.status;
          if (p.summary) s.summary = p.summary;
          subagents.set(p.subagent_id, s);
          renderClowder();
        }
      } else if (kind === 'Error') {
        if (!lastBot) return;
        lastBot.textContent = '❌ ' + (p.message || 'Turn failed.');
      }
      // TurnStarted / TurnFinished carry no visible content: the next
      // prompt opens a new bubble, which is the visible boundary.
      container.scrollTop = container.scrollHeight;
    }

    function statusLabel(status) {
      return { pending: 'menunggu', running: 'berjalan…', done: 'selesai ✓', failed: 'gagal ✗', interrupted: 'terhenti' }[status] || status || 'menunggu';
    }

    function renderClowder() {
      const listEl = document.getElementById('clowder-list');
      if (!listEl) return;
      listEl.innerHTML = '';
      if (subagents.size === 0) {
        listEl.innerHTML = '<div style="color: var(--text-muted); font-size: 0.85rem;">Tidak ada subagent aktif.</div>';
        return;
      }
      for (const [id, s] of subagents) {
        const item = document.createElement('div');
        item.className = 'clowder-item';
        const role = document.createElement('span');
        role.className = 'clowder-role';
        role.textContent = '🐾 ' + (s.role || 'subagent');
        const status = document.createElement('span');
        status.className = 'clowder-status-' + (s.status || 'pending');
        status.textContent = ' ' + statusLabel(s.status);
        const brief = document.createElement('div');
        brief.className = 'clowder-brief';
        brief.textContent = (s.summary || s.brief || id).slice(0, 120);
        item.append(role, status, brief);
        listEl.appendChild(item);
      }
    }

    function selectToolCall(callId) {
      selectedCallId = callId;
      document.querySelectorAll('.tool-card').forEach(c => c.classList.toggle('selected', c.dataset.callId === callId));
      renderInspector();
    }

    function renderInspector() {
      const box = document.getElementById('inspector-content');
      if (!box) return;
      const id = selectedCallId;
      const entry = id ? toolCalls.get(id) : null;
      if (!entry) {
        box.innerHTML = '<div style="color: var(--text-muted); font-size: 0.85rem;">Klik tool call untuk melihat detail.</div>';
        return;
      }
      let html = `<div><strong>🔧 ${escapeHtml(entry.tool || '')}</strong> <span style="color: var(--text-muted); font-size: 0.85rem;">${escapeHtml(entry.status || '')}</span></div>`;
      if (entry.description) html += `<h4>Description</h4><div style="font-size: 0.85rem;">${escapeHtml(entry.description)}</div>`;
      if (entry.arguments !== undefined && entry.arguments !== null) html += `<h4>Arguments</h4><pre>${escapeHtml(JSON.stringify(entry.arguments, null, 2))}</pre>`;
      if (entry.diffPreview) html += `<h4>Diff preview</h4><pre>${escapeHtml(entry.diffPreview)}</pre>`;
      if (entry.result !== undefined) html += `<h4>Result</h4><pre>${escapeHtml(typeof entry.result === 'string' ? entry.result : JSON.stringify(entry.result, null, 2))}</pre>`;
      box.innerHTML = html;
      if (entry.status === 'pending-approval') {
        const actions = document.createElement('div');
        actions.className = 'approval-actions';
        actions.style.marginTop = '12px';
        const yes = document.createElement('button');
        yes.className = 'btn btn-primary';
        yes.textContent = 'Terapkan';
        yes.addEventListener('click', () => resolveInspectorApproval(id, true));
        const no = document.createElement('button');
        no.className = 'btn btn-danger';
        no.textContent = 'Tolak';
        no.addEventListener('click', () => resolveInspectorApproval(id, false));
        actions.append(yes, no);
        box.appendChild(actions);
      }
    }

    async function resolveInspectorApproval(callId, approved) {
      const data = await postApproval(callId, approved);
      const entry = toolCalls.get(callId);
      if (entry) entry.status = data.resolved ? (approved ? 'approved' : 'denied') : 'expired';
      const banner = document.querySelector(`.approval-banner[data-call-id="${CSS.escape(callId)}"]`);
      if (banner) {
        banner.innerHTML = `<div>${data.resolved ? (approved ? 'Approval ✅ granted.' : 'Approval ❌ denied.') : 'No pending approval for this card (already resolved or expired).'}</div>`;
        banner.style.opacity = '0.7';
      }
      renderInspector();
    }

    async function postApproval(callId, approved) {
      const res = await fetch(apiUrl('/api/approval'), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ call_id: callId, approved, session_id: activeSession.id })
      });
      return await res.json().catch(() => ({}));
    }

    async function resolveApproval(callId, approved, btn) {
      const banner = btn ? btn.closest('.approval-banner') : null;
      try {
        const data = await postApproval(callId, approved);
        if (banner) {
          if (data.resolved) {
            banner.innerHTML = `<div>Approval ${approved ? '✅ granted' : '❌ denied'}.</div>`;
            banner.style.opacity = '0.7';
          } else {
            banner.innerHTML = `<div>No pending approval for this card (already resolved or expired).</div>`;
          }
        }
      } catch (e) {
        if (banner) banner.innerHTML = `<div>Failed to resolve approval: ${escapeHtml(String(e))}</div>`;
      }
    }

    document.getElementById('prompt-input').addEventListener('keydown', function(e) {
      if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        submitPrompt();
      }
    });

    document.getElementById('new-session-btn').addEventListener('click', () => createSession());

    window.onload = function() {
      loadSessions();
    };"#;
