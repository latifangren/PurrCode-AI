// SPDX-License-Identifier: GPL-3.0-or-later

//! The WebUI page: HTML document skeleton assembled from the CSS
//! (`html_css`) and JavaScript (`html_js`) constants. Split in v0.4
//! B1 so no file breaches the ADR-011 800-line limit: the document
//! is the format string, the style and script are arguments, so
//! their braces never reach `format!`.

use crate::web::html_css::WEB_INDEX_CSS;
use crate::web::html_js::WEB_INDEX_JS;

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
{}
  </script>
</body>
</html>"#,
        WEB_INDEX_CSS, WEB_INDEX_JS
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
}
