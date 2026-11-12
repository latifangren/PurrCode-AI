// SPDX-License-Identifier: GPL-3.0-or-later

pub const WEB_INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>PurrCode AI 🐾 Sovereign Multi-Agent Engineer</title>
  <style>
    :root {
      --bg: #11111b;
      --card-bg: #181825;
      --card-border: #313244;
      --text: #cdd6f4;
      --text-muted: #a6adc8;
      --primary: #cba6f7;
      --primary-hover: #b4befe;
      --accent: #f5c2e7;
      --success: #a6e3a1;
      --warning: #f9e2af;
      --danger: #f38ba8;
      --font-mono: 'JetBrains Mono', 'Fira Code', 'Courier New', monospace;
    }
    * { box-sizing: border-box; margin: 0; padding: 0; }
    body {
      background-color: var(--bg);
      color: var(--text);
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
      display: flex;
      height: 100vh;
      overflow: hidden;
    }
    /* Sidebar */
    .sidebar {
      width: 240px;
      background-color: var(--card-bg);
      border-right: 1px solid var(--card-border);
      display: flex;
      flex-direction: column;
      padding: 16px;
      flex-shrink: 0;
    }
    .logo {
      display: flex;
      align-items: center;
      gap: 10px;
      font-size: 1.2rem;
      font-weight: bold;
      color: var(--primary);
      margin-bottom: 24px;
    }
    .logo span { font-size: 1.6rem; }
    .nav {
      display: flex;
      flex-direction: column;
      gap: 6px;
      flex-grow: 1;
    }
    .nav-btn {
      background: none;
      border: none;
      color: var(--text-muted);
      padding: 10px 14px;
      border-radius: 8px;
      text-align: left;
      font-size: 0.95rem;
      cursor: pointer;
      display: flex;
      align-items: center;
      gap: 10px;
      transition: all 0.2s;
    }
    .nav-btn:hover {
      background-color: var(--card-border);
      color: var(--text);
    }
    .nav-btn.active {
      background-color: rgba(203, 166, 247, 0.15);
      color: var(--primary);
      font-weight: 600;
    }
    .status-badge {
      display: flex;
      align-items: center;
      gap: 8px;
      font-size: 0.8rem;
      color: var(--text-muted);
      padding-top: 12px;
      border-top: 1px solid var(--card-border);
    }
    .status-dot {
      width: 8px;
      height: 8px;
      border-radius: 50%;
      background-color: var(--success);
      box-shadow: 0 0 8px var(--success);
    }
    /* Main Content */
    .main {
      flex-grow: 1;
      display: flex;
      flex-direction: column;
      overflow: hidden;
    }
    .header {
      height: 56px;
      border-bottom: 1px solid var(--card-border);
      display: flex;
      align-items: center;
      justify-content: space-between;
      padding: 0 24px;
      background-color: var(--card-bg);
    }
    .header-title {
      font-size: 1.1rem;
      font-weight: 600;
    }
    .header-tags {
      display: flex;
      gap: 8px;
    }
    .tag {
      background-color: var(--card-border);
      font-size: 0.75rem;
      padding: 4px 10px;
      border-radius: 12px;
      color: var(--text-muted);
    }
    .tag.primary {
      background-color: rgba(203, 166, 247, 0.2);
      color: var(--primary);
    }
    .content {
      flex-grow: 1;
      overflow-y: auto;
      padding: 24px;
      display: flex;
      flex-direction: column;
      gap: 16px;
    }
    /* Chat View */
    .chat-view {
      display: flex;
      flex-direction: column;
      height: 100%;
    }
    .messages-container {
      flex-grow: 1;
      overflow-y: auto;
      display: flex;
      flex-direction: column;
      gap: 16px;
      padding-bottom: 16px;
    }
    .msg {
      display: flex;
      flex-direction: column;
      max-width: 85%;
      padding: 14px 18px;
      border-radius: 12px;
      font-size: 0.95rem;
      line-height: 1.5;
    }
    .msg.user {
      align-self: flex-end;
      background-color: rgba(203, 166, 247, 0.2);
      border: 1px solid rgba(203, 166, 247, 0.3);
      color: var(--text);
    }
    .msg.assistant {
      align-self: flex-start;
      background-color: var(--card-bg);
      border: 1px solid var(--card-border);
    }
    .msg-header {
      font-size: 0.75rem;
      font-weight: bold;
      color: var(--primary);
      margin-bottom: 6px;
      display: flex;
      align-items: center;
      gap: 6px;
    }
    .reasoning-box {
      background-color: rgba(49, 50, 68, 0.5);
      border-left: 3px solid var(--accent);
      padding: 8px 12px;
      font-size: 0.85rem;
      color: var(--text-muted);
      border-radius: 0 6px 6px 0;
      margin-bottom: 8px;
      white-space: pre-wrap;
      font-family: var(--font-mono);
    }
    .tool-card {
      background-color: #1e1e2e;
      border: 1px solid var(--card-border);
      border-radius: 8px;
      padding: 10px 14px;
      margin: 8px 0;
      font-family: var(--font-mono);
      font-size: 0.85rem;
    }
    .tool-header {
      color: var(--warning);
      font-weight: bold;
      display: flex;
      justify-content: space-between;
      margin-bottom: 4px;
    }
    .approval-banner {
      background-color: rgba(243, 139, 168, 0.15);
      border: 1px solid var(--danger);
      border-radius: 8px;
      padding: 12px 16px;
      display: flex;
      align-items: center;
      justify-content: space-between;
      margin: 12px 0;
    }
    .approval-actions {
      display: flex;
      gap: 8px;
    }
    .btn {
      padding: 8px 16px;
      border-radius: 6px;
      border: none;
      font-weight: 600;
      cursor: pointer;
      font-size: 0.9rem;
      transition: all 0.2s;
    }
    .btn-primary {
      background-color: var(--primary);
      color: #11111b;
    }
    .btn-primary:hover { background-color: var(--primary-hover); }
    .btn-danger {
      background-color: var(--danger);
      color: #11111b;
    }
    .btn-secondary {
      background-color: var(--card-border);
      color: var(--text);
    }
    .chat-input-bar {
      display: flex;
      gap: 12px;
      background-color: var(--card-bg);
      border: 1px solid var(--card-border);
      border-radius: 12px;
      padding: 8px 12px;
    }
    .chat-input {
      flex-grow: 1;
      background: none;
      border: none;
      color: var(--text);
      font-size: 0.95rem;
      outline: none;
      resize: none;
      min-height: 24px;
      max-height: 120px;
    }
    /* Cards and Table View */
    .view-card {
      background-color: var(--card-bg);
      border: 1px solid var(--card-border);
      border-radius: 10px;
      padding: 20px;
    }
    .view-card h3 {
      color: var(--primary);
      margin-bottom: 12px;
      font-size: 1.1rem;
    }
    table {
      width: 100%;
      border-collapse: collapse;
      font-size: 0.9rem;
      margin-top: 8px;
    }
    th, td {
      padding: 10px 14px;
      text-align: left;
      border-bottom: 1px solid var(--card-border);
    }
    th {
      color: var(--text-muted);
      font-weight: 600;
      background-color: rgba(49, 50, 68, 0.3);
    }
    pre {
      background-color: #1e1e2e;
      padding: 12px;
      border-radius: 8px;
      font-family: var(--font-mono);
      font-size: 0.85rem;
      overflow-x: auto;
      border: 1px solid var(--card-border);
    }
  </style>
</head>
<body>
  <div class="sidebar">
    <div class="logo">
      <span>🐾</span>
      <div>PurrCode AI</div>
    </div>
    <div class="nav">
      <button class="nav-btn active" onclick="switchTab('chat')">💬 Terminal & Chat</button>
      <button class="nav-btn" onclick="switchTab('cron')">⏰ Cron Scheduler</button>
      <button class="nav-btn" onclick="switchTab('skills')">🛠️ Skills & Curator</button>
      <button class="nav-btn" onclick="switchTab('memory')">🧠 Memory & Identity</button>
      <button class="nav-btn" onclick="switchTab('plugins')">🔌 Plugins & Sandbox</button>
    </div>
    <div class="status-badge">
      <div class="status-dot"></div>
      <span id="gateway-status">Gateway Connected</span>
    </div>
  </div>

  <div class="main">
    <div class="header">
      <div class="header-title" id="tab-title">Terminal & Chat</div>
      <div class="header-tags">
        <div class="tag primary" id="preset-badge">Preset: tabby</div>
        <div class="tag" id="model-badge">The Clowder Orchestrator</div>
      </div>
    </div>

    <!-- Chat Tab -->
    <div class="content chat-view" id="tab-chat">
      <div class="messages-container" id="messages">
        <div class="msg assistant">
          <div class="msg-header">🐾 PurrCode AI (Meowstro)</div>
          <div>Selamat datang di PurrCode AI WebUI! Sistem orkestrasi The Clowder dan Gateway aktif. Ada yang bisa saya bantu analisa atau kerjakan?</div>
        </div>
      </div>
      <div class="chat-input-bar">
        <textarea class="chat-input" id="prompt-input" rows="1" placeholder="Type a prompt for PurrCode AI... (Shift+Enter for newline)"></textarea>
        <button class="btn btn-primary" onclick="submitPrompt()">Send</button>
      </div>
    </div>

    <!-- Cron Tab -->
    <div class="content" id="tab-cron" style="display: none;">
      <div class="view-card">
        <h3>⏰ Autonomous Cron Jobs</h3>
        <p style="color: var(--text-muted); font-size: 0.9rem; margin-bottom: 16px;">
          Autonomous background jobs executing on a 60-second ticker with fail-closed tool allowlists.
        </p>
        <table id="cron-table">
          <thead>
            <tr>
              <th>ID</th>
              <th>Schedule</th>
              <th>Prompt</th>
              <th>Allowed Tools</th>
              <th>Next Run</th>
              <th>Status</th>
            </tr>
          </thead>
          <tbody id="cron-tbody">
            <tr><td colspan="6" style="text-align: center; color: var(--text-muted);">Loading scheduled jobs...</td></tr>
          </tbody>
        </table>
      </div>
    </div>

    <!-- Skills Tab -->
    <div class="content" id="tab-skills" style="display: none;">
      <div class="view-card">
        <h3>🛠️ Candidate Skills (Curator Engine)</h3>
        <p style="color: var(--text-muted); font-size: 0.9rem; margin-bottom: 16px;">
          Skills automatically proposed from complex trajectories (5+ tool calls) awaiting user approval.
        </p>
        <div id="candidates-container" style="display: flex; flex-direction: column; gap: 12px;">
          <div style="color: var(--text-muted);">Loading candidates...</div>
        </div>
      </div>
    </div>

    <!-- Memory Tab -->
    <div class="content" id="tab-memory" style="display: none;">
      <div class="view-card">
        <h3>🧠 Identity Memory (Hermes Parity)</h3>
        <p style="color: var(--text-muted); font-size: 0.9rem; margin-bottom: 16px;">
          Markdown-based identity files strictly confined by length invariants.
        </p>
        <div style="display: flex; gap: 8px; margin-bottom: 12px;">
          <button class="btn btn-secondary" onclick="loadMemoryDoc('SOUL.md')">SOUL.md</button>
          <button class="btn btn-secondary" onclick="loadMemoryDoc('USER.md')">USER.md</button>
          <button class="btn btn-secondary" onclick="loadMemoryDoc('MEMORY.md')">MEMORY.md</button>
        </div>
        <pre id="memory-content">Select a memory file above to view content.</pre>
      </div>
    </div>

    <!-- Plugins Tab -->
    <div class="content" id="tab-plugins" style="display: none;">
      <div class="view-card">
        <h3>🔌 Plugin Manifests & Sandboxing</h3>
        <p style="color: var(--text-muted); font-size: 0.9rem; margin-bottom: 16px;">
          Dynamic plugins loaded with default-deny capability sandboxes.
        </p>
        <pre id="plugins-content">Loading installed plugins and sandboxes...</pre>
      </div>
    </div>
  </div>

  <script>
    let activeTab = 'chat';

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
          body: JSON.stringify({ prompt: text })
        });
      } catch (e) {
        botMsg.querySelector('.bot-content').textContent = 'Error: ' + e;
      }
    }

    function escapeHtml(str) {
      return str.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&#39;');
    }

    // Connect Server-Sent Events (SSE)
    function setupSSE() {
      const es = new EventSource(apiUrl('/api/events'));
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
        if (!lastBot) return;
        const card = document.createElement('div');
        card.className = 'tool-card';
        card.dataset.callId = p.call_id || '';
        card.innerHTML = `<div class="tool-header"><span>🔧 ${escapeHtml(p.tool || '')}</span><span>Executing...</span></div><pre>${escapeHtml(JSON.stringify(p.arguments, null, 2))}</pre>`;
        lastBot.parentElement.insertBefore(card, lastBot);
      } else if (kind === 'ToolCallFinished') {
        const card = p.call_id ? container.querySelector(`.tool-card[data-call-id="${CSS.escape(p.call_id)}"]`) : null;
        if (card) {
          const status = card.querySelector('.tool-header span:last-child');
          if (status) status.textContent = p.success ? 'Done ✓' : 'Failed ✗';
        }
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
      } else if (kind === 'Error') {
        if (!lastBot) return;
        lastBot.textContent = '❌ ' + (p.message || 'Turn failed.');
      }
      // TurnStarted / TurnFinished carry no visible content: the next
      // prompt opens a new bubble, which is the visible boundary.
      container.scrollTop = container.scrollHeight;
    }

    async function resolveApproval(callId, approved, btn) {
      const banner = btn ? btn.closest('.approval-banner') : null;
      try {
        const res = await fetch(apiUrl('/api/approval'), {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ call_id: callId, approved })
        });
        const data = await res.json().catch(() => ({}));
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

    window.onload = function() {
      setupSSE();
    };
  </script>
</body>
</html>
"#;
