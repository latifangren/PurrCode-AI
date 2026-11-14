// SPDX-License-Identifier: GPL-3.0-or-later

//! The WebUI page: HTML document skeleton assembled from the CSS
//! (`html_css`) and JavaScript (`html_js`) constants. Split in v0.4
//! B1 so no file breaches the ADR-011 800-line limit: the document
//! is the format string, the style and script are arguments, so
//! their braces never reach `format!`.

use crate::web::html_css::WEB_INDEX_CSS;
use crate::web::html_js::WEB_INDEX_JS;
use crate::web::html_js_b5::WEB_INDEX_JS_B5;
use crate::web::html_js_b6::WEB_INDEX_JS_B6;
use crate::web::html_js_b7::WEB_INDEX_JS_B7;

pub fn web_index_html() -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>PurrCode AI 🐾 Sovereign Multi-Agent Engineer</title>
  <style>
{}
  </style>
</head>
<body>
  <div class="sidebar-backdrop" id="sidebar-backdrop"></div>
  <div class="sidebar">
    <div class="logo">
      <span>🐾</span>
      <div>PurrCode AI</div>
    </div>
    <div class="nav">
      <button class="nav-btn active" onclick="switchTab('chat')">💬 Terminal & Chat</button>
      <button class="nav-btn" onclick="switchTab('overview')">📊 Overview</button>
      <button class="nav-btn" onclick="switchTab('cron')">⏰ Cron Scheduler</button>
      <button class="nav-btn" onclick="switchTab('skills')">🛠️ Skills & Curator</button>
      <button class="nav-btn" onclick="switchTab('memory')">🧠 Memory & Identity</button>
      <button class="nav-btn" onclick="switchTab('plugins')">🔌 Plugins & Sandbox</button>
    </div>
    <div class="sessions-section">
      <div class="sessions-header">
        <span>Sesi</span>
        <button class="btn btn-secondary sessions-new" id="new-session-btn" title="Sesi baru">＋</button>
      </div>
      <div id="session-list">
        <div style="color: var(--text-muted); font-size: 0.85rem;">Loading sessions...</div>
      </div>
    </div>
    <div class="clowder-section">
      <div class="clowder-header">🐾 Clowder — live</div>
      <div id="clowder-list">
        <div style="color: var(--text-muted); font-size: 0.85rem;">Tidak ada subagent aktif.</div>
      </div>
    </div>
    <div class="status-badge">
      <div class="status-dot"></div>
      <span id="gateway-status">Gateway Connected</span>
    </div>
  </div>

  <div class="main">
    <div class="header">
      <button class="menu-btn" id="menu-btn" aria-label="Open menu">☰</button>
      <div class="header-title" id="tab-title">Terminal & Chat</div>
      <div class="header-tags">
        <div class="tag primary" id="preset-badge">Preset: tabby</div>
        <div class="tag" id="model-badge">The Clowder Orchestrator</div>
      </div>
    </div>

    <!-- Chat Tab -->
    <div class="content chat-view" id="tab-chat">
      <div class="chat-body">
        <div class="messages-container" id="messages">
          <div class="msg assistant">
            <div class="msg-header">🐾 PurrCode AI (Meowstro)</div>
            <div>Selamat datang di PurrCode AI WebUI! Sistem orkestrasi The Clowder dan Gateway aktif. Ada yang bisa saya bantu analisa atau kerjakan?</div>
          </div>
        </div>
        <aside class="inspector" id="inspector">
          <div class="inspector-header">🔍 Inspector</div>
          <div id="inspector-content">
            <div style="color: var(--text-muted); font-size: 0.85rem;">Klik tool call untuk melihat detail.</div>
          </div>
        </aside>
      </div>
      <div class="chat-input-bar">
        <textarea class="chat-input" id="prompt-input" rows="1" placeholder="Type a prompt for PurrCode AI... (Shift+Enter for newline)"></textarea>
        <button class="btn btn-primary" onclick="submitPrompt()">Send</button>
      </div>
    </div>

    <!-- Overview Tab (v0.4 B7: dashboard canvas) -->
    <div class="content" id="tab-overview" style="display: none;">
      <div id="overview-warnings" class="ov-warn" style="display: none;"></div>
      <div class="ov-grid">
        <div class="view-card ov-card">
          <div class="ov-label">Gateway</div>
          <div class="ov-kpi" id="ov-gateway-status">…</div>
          <div style="color: var(--text-muted); font-size: 0.85rem;" id="ov-gateway-workspace"></div>
        </div>
        <div class="view-card ov-card">
          <div class="ov-label">Sesi aktif</div>
          <div class="ov-kpi" id="ov-sessions-count">…</div>
          <div id="ov-sessions-list" style="margin-top: 8px; display: flex; flex-direction: column; gap: 4px;"></div>
        </div>
        <div class="view-card ov-card">
          <div class="ov-label">Cron berikutnya</div>
          <div id="ov-next-cron" style="margin-top: 8px; font-size: 0.9rem;">…</div>
        </div>
        <div class="view-card ov-card">
          <div class="ov-label">Delivery terakhir</div>
          <div id="ov-last-delivery" style="margin-top: 8px; font-size: 0.9rem;">…</div>
        </div>
      </div>
      <div><button class="btn btn-secondary" id="ov-refresh-btn">Refresh</button></div>
    </div>

    <!-- Cron Tab -->
    <div class="content" id="tab-cron" style="display: none;">
      <div class="view-card">
        <h3>⏰ Autonomous Cron Jobs</h3>
        <p style="color: var(--text-muted); font-size: 0.9rem; margin-bottom: 16px;">
          Autonomous background jobs executing on a 60-second ticker with fail-closed tool allowlists.
        </p>
        <div style="display: flex; flex-direction: column; gap: 8px; margin-bottom: 16px;">
          <input id="cron-add-name" placeholder="name (optional — auto-generated when empty)" style="padding: 8px; background: var(--bg-secondary); color: var(--text-primary); border: 1px solid var(--border-color); border-radius: 6px;">
          <input id="cron-add-schedule" placeholder="schedule — e.g. tiap pagi jam 8, every Monday 09:00" style="padding: 8px; background: var(--bg-secondary); color: var(--text-primary); border: 1px solid var(--border-color); border-radius: 6px;">
          <textarea id="cron-add-prompt" rows="3" placeholder="What should the job do on every firing?" style="padding: 8px; background: var(--bg-secondary); color: var(--text-primary); border: 1px solid var(--border-color); border-radius: 6px;"></textarea>
          <input id="cron-add-allow" placeholder="allowed tools, comma-separated (empty = Meowstro read-only default; blank list = nothing allowed)" style="padding: 8px; background: var(--bg-secondary); color: var(--text-primary); border: 1px solid var(--border-color); border-radius: 6px;">
          <input id="cron-add-preset" placeholder="preset (optional — config default; never oyen/yolo)" style="padding: 8px; background: var(--bg-secondary); color: var(--text-primary); border: 1px solid var(--border-color); border-radius: 6px;">
          <div style="display: flex; gap: 8px;">
            <button class="btn btn-secondary" id="cron-parse-btn">Parse schedule</button>
            <button class="btn btn-primary" id="cron-add-btn">Create job</button>
          </div>
          <div id="cron-parse-preview" style="color: var(--text-muted); font-size: 0.9rem;"></div>
          <div id="cron-add-status" style="font-size: 0.9rem;"></div>
        </div>
        <div class="table-wrap">
        <table id="cron-table">
          <thead>
            <tr>
              <th>Job</th>
              <th>Schedule</th>
              <th>Prompt</th>
              <th>Allowed Tools</th>
              <th>Next Run</th>
              <th>Status</th>
              <th>Actions</th>
            </tr>
          </thead>
          <tbody id="cron-tbody">
            <tr><td colspan="7" style="text-align: center; color: var(--text-muted);">Loading scheduled jobs...</td></tr>
          </tbody>
        </table>
        </div>
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
      <div class="view-card">
        <h3>📚 All Skills</h3>
        <p style="color: var(--text-muted); font-size: 0.9rem; margin-bottom: 16px;">
          Every skill the manager knows: 8 bundled + global + project. Embedded skills are
          read-only — edit one by creating a project/global copy of the same name, which shadows it.
        </p>
        <div id="skills-all-container" style="display: flex; flex-direction: column; gap: 12px;">
          <div style="color: var(--text-muted);">Loading skills...</div>
        </div>
        <div id="skills-status" style="color: var(--text-muted); font-size: 0.9rem; margin-top: 12px;"></div>
      </div>
      <div class="view-card">
        <h3>➕ New Skill</h3>
        <p style="color: var(--text-muted); font-size: 0.9rem; margin-bottom: 16px;">
          Created through <code>manage_skill</code> — the same tool a Tinker turn uses.
          Frontmatter is added when the body has none.
        </p>
        <div style="display: flex; flex-direction: column; gap: 8px;">
          <input id="skill-add-name" placeholder="name (lowercase-hyphen)" style="padding: 8px; background: var(--bg-secondary); color: var(--text-primary); border: 1px solid var(--border-color); border-radius: 6px;">
          <input id="skill-add-description" placeholder="One-sentence routing description" style="padding: 8px; background: var(--bg-secondary); color: var(--text-primary); border: 1px solid var(--border-color); border-radius: 6px;">
          <select id="skill-add-scope" style="padding: 8px; background: var(--bg-secondary); color: var(--text-primary); border: 1px solid var(--border-color); border-radius: 6px;">
            <option value="project">project (.purrcode/skills/)</option>
            <option value="global">global (~/.purrcode/skills/)</option>
          </select>
          <textarea id="skill-add-content" rows="8" placeholder="Full SKILL.md body" style="padding: 8px; background: var(--bg-secondary); color: var(--text-primary); border: 1px solid var(--border-color); border-radius: 6px; font-family: monospace;"></textarea>
          <div><button class="btn btn-primary" id="skill-add-btn">Create Skill</button></div>
        </div>
      </div>
      <div class="view-card" id="skill-edit-card" style="display: none;">
        <h3>✏️ Edit Skill <span id="skill-edit-scope" style="color: var(--text-muted); font-size: 0.85rem;"></span></h3>
        <p style="color: var(--text-muted); font-size: 0.9rem; margin-bottom: 16px;">
          The body below is the whole SKILL.md file — edit the frontmatter to change the
          description. Saved through <code>manage_skill</code>.
        </p>
        <div style="display: flex; flex-direction: column; gap: 8px;">
          <input id="skill-edit-name" readonly style="padding: 8px; background: var(--bg-secondary); color: var(--text-muted); border: 1px solid var(--border-color); border-radius: 6px;">
          <textarea id="skill-edit-content" rows="12" style="padding: 8px; background: var(--bg-secondary); color: var(--text-primary); border: 1px solid var(--border-color); border-radius: 6px; font-family: monospace;"></textarea>
          <div style="display: flex; gap: 8px;">
            <button class="btn btn-primary" id="skill-edit-save">Save</button>
            <button class="btn btn-secondary" id="skill-edit-cancel">Cancel</button>
          </div>
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
        <div id="memory-info" style="color: var(--text-muted); font-size: 0.85rem; margin-bottom: 12px;">Loading backend info...</div>
        <div style="display: flex; gap: 8px; margin-bottom: 12px;">
          <button class="btn btn-secondary" onclick="loadMemoryDoc('SOUL.md')">SOUL.md</button>
          <button class="btn btn-secondary" onclick="loadMemoryDoc('USER.md')">USER.md</button>
          <button class="btn btn-secondary" onclick="loadMemoryDoc('MEMORY.md')">MEMORY.md</button>
        </div>
        <pre id="memory-content">Select a memory file above to view content.</pre>
        <textarea id="memory-editor" rows="12" style="width: 100%; margin-top: 12px; padding: 8px; background: var(--bg-secondary); color: var(--text-primary); border: 1px solid var(--border-color); border-radius: 6px; font-family: monospace;" placeholder="Edit the loaded file here, then Save."></textarea>
        <div style="display: flex; gap: 8px; margin-top: 12px; align-items: center;">
          <button class="btn btn-primary" id="memory-save-btn">Save</button>
          <span id="memory-status" style="color: var(--text-muted); font-size: 0.9rem;"></span>
        </div>
        <p style="color: var(--text-muted); font-size: 0.85rem; margin-top: 12px;">
          Saves are validated (100K characters, no NUL bytes) and audited as
          <code>memory_save</code>. Only the three global files are writable here;
          the project-local <code>.purrcode/MEMORY.md</code> is read-only from the WebUI.
        </p>
      </div>
    </div>

    <!-- Plugins Tab -->
    <div class="content" id="tab-plugins" style="display: none;">
      <div class="view-card">
        <h3>🔌 Plugins</h3>
        <p style="color: var(--text-muted); font-size: 0.9rem; margin-bottom: 16px;">
          Mount or unmount a plugin without restarting the server. A mounted plugin's hooks
          run on every turn. Only secrets are default-deny: a process plugin runs with full
          user rights, and network/filesystem capabilities are declarations, not enforcement.
        </p>
        <div id="plugins-container">Loading installed plugins...</div>
        <div id="plugins-status" class="plugins-status"></div>
      </div>
    </div>
  </div>

  <script>
{}
{}
{}
{}
  </script>
</body>
</html>"#,
        WEB_INDEX_CSS, WEB_INDEX_JS, WEB_INDEX_JS_B5, WEB_INDEX_JS_B6, WEB_INDEX_JS_B7
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::html_js::WEB_INDEX_JS;

    /// v0.4 B3: the inspector column and the live Clowder list must
    /// exist in the served page, and the JS must wire them without
    /// inline handlers — call ids and briefs are model-controlled
    /// and must never be interpolated into an HTML attribute or a
    /// JS string literal (same tripwire rule as the R3 approval
    /// cards in server/tests.rs).
    #[test]
    fn test_webui_has_inspector_and_clowder_panel() {
        let html = web_index_html();
        assert!(
            html.contains("id=\"inspector\""),
            "inspector column missing"
        );
        assert!(
            html.contains("id=\"inspector-content\""),
            "inspector content box missing"
        );
        assert!(html.contains("id=\"clowder-list\""), "clowder list missing");
        assert!(
            html.contains("class=\"chat-body\""),
            "chat-body wrapper (transcript + inspector) missing"
        );
        assert!(
            !WEB_INDEX_JS.contains("onclick=\""),
            "JS must not use inline onclick"
        );
        assert!(
            WEB_INDEX_JS.contains("selectToolCall"),
            "tool cards must wire a click-to-inspect handler"
        );
        assert!(
            WEB_INDEX_JS.contains("SubagentSpawned"),
            "JS must handle the SubagentSpawned bus event"
        );
        assert!(
            WEB_INDEX_JS.contains("SubagentStatusChanged"),
            "JS must handle the SubagentStatusChanged bus event"
        );
    }

    /// v0.4 B4: the plugins tab renders cards (not a JSON dump) with
    /// a Mount/Unmount button per plugin, wired via `dataset` +
    /// `addEventListener` — plugin names come from manifest files
    /// and must never be interpolated into an inline handler.
    #[test]
    fn test_webui_plugins_tab_has_mount_controls() {
        let html = web_index_html();
        assert!(
            html.contains("id=\"plugins-container\""),
            "plugins container missing"
        );
        assert!(
            html.contains("id=\"plugins-status\""),
            "plugins status line missing"
        );
        assert!(
            !html.contains("id=\"plugins-content\""),
            "old JSON-dump <pre> must be gone"
        );
        assert!(
            WEB_INDEX_JS.contains("/api/plugins/mount"),
            "JS must call the mount endpoint"
        );
        assert!(
            WEB_INDEX_JS.contains("/api/plugins/unmount"),
            "JS must call the unmount endpoint"
        );
        assert!(
            WEB_INDEX_JS.contains("dataset.pluginName"),
            "plugin name must travel via dataset, not an inline handler"
        );
        assert!(
            WEB_INDEX_JS.contains("plugin-badge"),
            "JS must render the mounted badge"
        );
        assert!(
            !WEB_INDEX_JS.contains("plugins-content"),
            "JS must not reference the removed <pre>"
        );
    }

    /// v0.4 B5: the memory tab shows the backend facts, an editor,
    /// and a Save button; the skills tab lists every skill with an
    /// add form and an edit form. The B5 JavaScript lives in
    /// `html_js_b5.rs` (concatenated into the same script).
    #[test]
    fn test_webui_b5_memory_and_skill_controls() {
        use crate::web::html_js_b5::WEB_INDEX_JS_B5;
        let html = web_index_html();
        assert!(
            html.contains("id=\"memory-info\""),
            "memory info line missing"
        );
        assert!(
            html.contains("id=\"memory-editor\""),
            "memory editor missing"
        );
        assert!(
            html.contains("id=\"memory-save-btn\""),
            "memory save button missing"
        );
        assert!(
            html.contains("id=\"skills-all-container\""),
            "all-skills container missing"
        );
        assert!(
            html.contains("id=\"skill-add-name\""),
            "skill add form missing"
        );
        assert!(
            html.contains("id=\"skill-edit-card\""),
            "skill edit card missing"
        );
        assert!(
            WEB_INDEX_JS_B5.contains("/api/memory/info"),
            "JS must call the memory info endpoint"
        );
        assert!(
            WEB_INDEX_JS_B5.contains("/api/skills/all"),
            "JS must call the all-skills endpoint"
        );
        assert!(
            WEB_INDEX_JS_B5.contains("/api/skills/create"),
            "JS must call the skill create endpoint"
        );
        assert!(
            WEB_INDEX_JS_B5.contains("/api/skills/update"),
            "JS must call the skill update endpoint"
        );
        assert!(
            WEB_INDEX_JS_B5.contains("/api/skills/delete"),
            "JS must call the skill delete endpoint"
        );
        assert!(
            WEB_INDEX_JS_B5.contains("dataset.skillName"),
            "skill name must travel via dataset, not an inline handler"
        );
        assert!(
            !WEB_INDEX_JS_B5.contains("onclick=\""),
            "B5 JS must not use inline onclick handlers"
        );
    }

    /// v0.4 B6: the cron tab has an add form (name, schedule,
    /// prompt, allowlist, preset) with a parse preview + Create
    /// button, and an Actions column; the B6 JavaScript calls
    /// the five scheduler endpoints and dispatches row buttons
    /// via `dataset`, never an inline handler.
    #[test]
    fn test_webui_b6_cron_scheduler_controls() {
        use crate::web::html_js_b6::WEB_INDEX_JS_B6;
        let html = web_index_html();
        assert!(
            html.contains("id=\"cron-add-name\""),
            "cron add form (name) missing"
        );
        assert!(
            html.contains("id=\"cron-add-schedule\""),
            "cron add form (schedule) missing"
        );
        assert!(
            html.contains("id=\"cron-add-prompt\""),
            "cron add form (prompt) missing"
        );
        assert!(
            html.contains("id=\"cron-add-allow\""),
            "cron add form (allowlist) missing"
        );
        assert!(
            html.contains("id=\"cron-parse-btn\""),
            "cron parse button missing"
        );
        assert!(
            html.contains("id=\"cron-add-btn\""),
            "cron create button missing"
        );
        assert!(
            html.contains("<th>Actions</th>"),
            "cron table Actions column missing"
        );
        assert!(
            WEB_INDEX_JS.contains("data-cron-action"),
            "cron rows must carry the action in a data attribute"
        );
        assert!(
            WEB_INDEX_JS_B6.contains("/api/cron/parse"),
            "JS must call the cron parse endpoint"
        );
        assert!(
            WEB_INDEX_JS_B6.contains("/api/cron/add"),
            "JS must call the cron add endpoint"
        );
        assert!(
            WEB_INDEX_JS_B6.contains("/api/cron/enable"),
            "JS must call the cron enable endpoint"
        );
        assert!(
            WEB_INDEX_JS_B6.contains("/api/cron/disable"),
            "JS must call the cron disable endpoint"
        );
        assert!(
            WEB_INDEX_JS_B6.contains("/api/cron/remove"),
            "JS must call the cron remove endpoint"
        );
        assert!(
            WEB_INDEX_JS_B6.contains("dataset.cronId"),
            "cron id must travel via dataset, not an inline handler"
        );
        assert!(
            !WEB_INDEX_JS_B6.contains("onclick=\""),
            "B6 JS must not use inline onclick handlers"
        );
    }

    /// v0.4 B7: the overview tab (dashboard canvas) with its four
    /// cards, the mobile drawer (hamburger + backdrop), and the
    /// scrollable cron table wrapper. The B7 JavaScript calls
    /// `/api/overview` and wires the drawer via `addEventListener`
    /// — never an inline handler — and renders server values
    /// through textContent, never innerHTML interpolation.
    #[test]
    fn test_webui_b7_overview_and_mobile_drawer() {
        use crate::web::html_css::WEB_INDEX_CSS;
        use crate::web::html_js_b7::WEB_INDEX_JS_B7;
        let html = web_index_html();
        assert!(html.contains("id=\"tab-overview\""), "overview tab missing");
        assert!(
            html.contains("id=\"ov-gateway-status\""),
            "gateway card missing"
        );
        assert!(
            html.contains("id=\"ov-sessions-count\""),
            "sessions card missing"
        );
        assert!(
            html.contains("id=\"ov-next-cron\""),
            "next-cron card missing"
        );
        assert!(
            html.contains("id=\"ov-last-delivery\""),
            "last-delivery card missing"
        );
        assert!(
            html.contains("id=\"ov-refresh-btn\""),
            "overview refresh button missing"
        );
        assert!(html.contains("id=\"menu-btn\""), "hamburger button missing");
        assert!(
            html.contains("id=\"sidebar-backdrop\""),
            "drawer backdrop missing"
        );
        assert!(
            html.contains("class=\"table-wrap\""),
            "cron table must sit in a scrollable wrapper on mobile"
        );
        assert!(
            html.contains("switchTab('overview')"),
            "overview nav button missing"
        );
        assert!(
            WEB_INDEX_CSS.contains(".sidebar.open"),
            "CSS must slide the drawer in"
        );
        assert!(
            WEB_INDEX_CSS.contains("@media (max-width: 900px)"),
            "CSS must have the drawer breakpoint"
        );
        assert!(
            WEB_INDEX_CSS.contains(".ov-grid"),
            "CSS must lay the dashboard cards out in a grid"
        );
        assert!(
            WEB_INDEX_JS_B7.contains("/api/overview"),
            "JS must call the overview endpoint"
        );
        assert!(
            WEB_INDEX_JS_B7.contains("loadOverview"),
            "JS must load the overview on tab switch"
        );
        assert!(
            WEB_INDEX_JS_B7.contains("toggleSidebar"),
            "JS must toggle the drawer"
        );
        assert!(
            WEB_INDEX_JS_B7.contains("dataset.sessionId"),
            "overview session rows must carry the id in a data attribute"
        );
        assert!(
            !WEB_INDEX_JS_B7.contains("onclick=\""),
            "B7 JS must not use inline onclick handlers"
        );
        assert!(
            !WEB_INDEX_JS_B7.contains("innerHTML = `<"),
            "B7 JS must not interpolate values into innerHTML"
        );
    }
}
