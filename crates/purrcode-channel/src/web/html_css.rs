// SPDX-License-Identifier: GPL-3.0-or-later

//! The WebUI stylesheet (extracted from `html.rs` in v0.4 B1).
//! Plain CSS text; the document in `html.rs` splices it into the
//! `<style>` element.

pub const WEB_INDEX_CSS: &str = r#"    :root {
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
    /* Sessions (v0.4 B1): sidebar list under the nav. */
    .sessions-section {
      margin-top: 20px;
      padding-top: 16px;
      border-top: 1px solid var(--card-border);
    }
    .sessions-header {
      display: flex;
      justify-content: space-between;
      align-items: center;
      padding: 0 8px;
      margin-bottom: 8px;
      font-weight: 600;
      font-size: 0.85rem;
      color: var(--text-muted);
      text-transform: uppercase;
      letter-spacing: 0.05em;
    }
    .sessions-new {
      padding: 2px 10px;
      font-size: 1rem;
      line-height: 1.2;
    }
    .session-item {
      display: block;
      width: 100%;
      text-align: left;
      padding: 8px 10px;
      margin-bottom: 4px;
      background: transparent;
      border: 1px solid transparent;
      border-radius: 8px;
      color: var(--text-primary);
      cursor: pointer;
    }
    .session-item:hover {
      background-color: rgba(49, 50, 68, 0.3);
    }
    .session-item.active {
      background-color: var(--accent-dim);
      border-color: var(--accent);
    }
    .session-item .session-title {
      display: block;
      font-size: 0.85rem;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    .session-item .session-meta {
      display: block;
      font-size: 0.75rem;
      color: var(--text-muted);
    }
    .msg.tool {
      opacity: 0.75;
      font-size: 0.85rem;
    }
    /* Chat body: transcript + inspector side by side (v0.4 B3). */
    .chat-body {
      flex-grow: 1;
      display: flex;
      gap: 16px;
      min-height: 0;
    }
    .inspector {
      width: 320px;
      flex-shrink: 0;
      background-color: var(--card-bg);
      border: 1px solid var(--card-border);
      border-radius: 10px;
      padding: 14px;
      overflow-y: auto;
      font-size: 0.9rem;
    }
    .inspector-header {
      font-weight: 600;
      color: var(--primary);
      margin-bottom: 10px;
    }
    .inspector h4 {
      color: var(--warning);
      margin: 12px 0 4px;
      font-size: 0.85rem;
    }
    .inspector pre {
      font-size: 0.8rem;
      max-height: 260px;
      overflow-y: auto;
    }
    .tool-card { cursor: pointer; }
    .tool-card.selected {
      border-color: var(--primary);
      box-shadow: 0 0 0 1px var(--primary);
    }
    /* Clowder — live subagent list in the sidebar (v0.4 B3). */
    .clowder-section {
      margin-top: 16px;
      padding-top: 12px;
      border-top: 1px solid var(--card-border);
    }
    .clowder-header {
      padding: 0 8px;
      margin-bottom: 8px;
      font-weight: 600;
      font-size: 0.85rem;
      color: var(--text-muted);
      text-transform: uppercase;
      letter-spacing: 0.05em;
    }
    .clowder-item {
      padding: 6px 8px;
      border-radius: 8px;
      margin-bottom: 4px;
      font-size: 0.85rem;
      background: rgba(49, 50, 68, 0.3);
    }
    .clowder-item .clowder-role { font-weight: 600; }
    .clowder-item .clowder-brief {
      color: var(--text-muted);
      font-size: 0.8rem;
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    .clowder-status-running { color: var(--warning); }
    .clowder-status-done { color: var(--success); }
    .clowder-status-failed { color: var(--danger); }
    .clowder-status-pending { color: var(--text-muted); }
    .clowder-status-interrupted { color: var(--text-muted); }
    /* Plugins tab (v0.4 B4) */
    .plugin-card {
      border: 1px solid var(--card-border);
      border-radius: 8px;
      padding: 12px;
      margin-bottom: 10px;
      background: rgba(49, 50, 68, 0.3);
    }
    .plugin-header {
      display: flex;
      align-items: baseline;
      gap: 8px;
      flex-wrap: wrap;
    }
    .plugin-name { font-weight: 600; }
    .plugin-version { color: var(--text-muted); font-size: 0.85rem; }
    .plugin-badge {
      font-size: 0.75rem;
      font-weight: 600;
      padding: 2px 8px;
      border-radius: 999px;
    }
    .plugin-badge.mounted { color: var(--success); background: rgba(166, 227, 161, 0.15); }
    .plugin-badge.unmounted { color: var(--text-muted); background: rgba(166, 173, 200, 0.15); }
    .plugin-desc { color: var(--text); font-size: 0.9rem; margin-top: 6px; }
    .plugin-meta {
      color: var(--text-muted);
      font-size: 0.8rem;
      font-family: var(--font-mono);
      word-break: break-all;
      margin-top: 6px;
    }
    .plugin-caps { color: var(--text-muted); font-size: 0.85rem; margin-top: 4px; }
    .plugin-warn { color: var(--warning); font-size: 0.85rem; margin-top: 6px; }
    .plugin-action {
      margin-top: 8px;
      background-color: var(--primary);
      color: #11111b;
      border: none;
      border-radius: 6px;
      padding: 6px 14px;
      font-size: 0.9rem;
      cursor: pointer;
    }
    .plugin-action:disabled { opacity: 0.5; cursor: default; }
    .plugins-status { color: var(--text-muted); font-size: 0.85rem; margin-top: 8px; min-height: 1.2em; }
    @media (max-width: 900px) {
      .inspector { display: none; }
    }"#;
