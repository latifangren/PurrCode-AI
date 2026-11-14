// SPDX-License-Identifier: GPL-3.0-or-later

//! The WebUI JavaScript for v0.4 B7 (mobile drawer + dashboard).
//! Plain JS text; the document in `html.rs` splices it into the
//! `<script>` element after the B5/B6 chunks, so `apiUrl`,
//! `escapeHtml`, `switchTab` and `switchSession` from `html_js.rs`
//! are already defined when this runs.

pub const WEB_INDEX_JS_B7: &str = r#"    // Mobile drawer (v0.4 B7): below 900px the sidebar leaves
    // the flex flow and slides in as an overlay; the hamburger
    // in the header opens it, the backdrop / Esc / any sidebar
    // button closes it. On desktop the button is hidden by CSS
    // and these handlers are inert (the sidebar has no .open).
    function isDrawerMode() {
      return window.matchMedia('(max-width: 900px)').matches;
    }
    function openSidebar() {
      document.querySelector('.sidebar').classList.add('open');
      document.getElementById('sidebar-backdrop').classList.add('show');
    }
    function closeSidebar() {
      document.querySelector('.sidebar').classList.remove('open');
      document.getElementById('sidebar-backdrop').classList.remove('show');
    }
    function toggleSidebar() {
      if (document.querySelector('.sidebar').classList.contains('open')) closeSidebar();
      else openSidebar();
    }

    // Dashboard canvas (v0.4 B7): one GET /api/overview fills
    // the four cards. Values are rendered through textContent /
    // createElement — session titles come from user messages and
    // must never be interpolated into HTML (same rule as the B3
    // tool cards). Reloaded on every tab switch plus the Refresh
    // button; no timer, so a hidden tab never polls.
    async function loadOverview() {
      const warnEl = document.getElementById('overview-warnings');
      try {
        const res = await fetch(apiUrl('/api/overview'));
        if (!res.ok) throw new Error('HTTP ' + res.status);
        renderOverview(await res.json());
      } catch (e) {
        if (warnEl) {
          warnEl.style.display = 'block';
          warnEl.textContent = 'Failed to load overview: ' + e.message;
        }
      }
    }

    function renderOverview(data) {
      const warnEl = document.getElementById('overview-warnings');
      const warnings = data.warnings || [];
      if (warnings.length) {
        warnEl.style.display = 'block';
        warnEl.textContent = '⚠️ ' + warnings.join(' · ');
      } else {
        warnEl.style.display = 'none';
        warnEl.textContent = '';
      }

      const gw = data.gateway || {};
      document.getElementById('ov-gateway-status').textContent = gw.status || 'unknown';
      document.getElementById('ov-gateway-workspace').textContent = gw.workspace || '';

      const sessions = data.sessions || {};
      document.getElementById('ov-sessions-count').textContent = String(sessions.active || 0);
      const listEl = document.getElementById('ov-sessions-list');
      listEl.innerHTML = '';
      for (const s of (sessions.items || [])) {
        const row = document.createElement('button');
        row.className = 'session-item';
        row.dataset.sessionId = s.id;
        const title = document.createElement('span');
        title.className = 'session-title';
        title.textContent = s.title || s.id;
        const meta = document.createElement('span');
        meta.className = 'session-meta';
        meta.textContent = s.id + ' · ' + (s.message_count || 0) + ' pesan';
        row.append(title, meta);
        row.addEventListener('click', () => {
          closeSidebar();
          switchTab('chat');
          switchSession(s.id);
        });
        listEl.appendChild(row);
      }
      if (!listEl.children.length) {
        listEl.innerHTML = '<div style="color: var(--text-muted); font-size: 0.85rem;">Belum ada sesi.</div>';
      }

      const nextEl = document.getElementById('ov-next-cron');
      nextEl.innerHTML = '';
      if (data.next_cron) {
        const name = document.createElement('div');
        const strong = document.createElement('strong');
        strong.textContent = data.next_cron.name || data.next_cron.id || '';
        name.appendChild(strong);
        const sched = document.createElement('div');
        sched.style.color = 'var(--text-muted)';
        sched.style.fontSize = '0.85rem';
        sched.textContent = (data.next_cron.schedule || '') + ' · ' + (data.next_cron.next_run || '');
        nextEl.append(name, sched);
      } else {
        nextEl.textContent = 'Tidak ada job cron aktif.';
      }

      const delEl = document.getElementById('ov-last-delivery');
      delEl.innerHTML = '';
      if (data.last_delivery) {
        const d = data.last_delivery;
        const job = document.createElement('div');
        const strong = document.createElement('strong');
        strong.textContent = d.job_name || d.job_id || '';
        job.appendChild(strong);
        const sum = document.createElement('div');
        sum.textContent = d.summary || '';
        const meta = document.createElement('div');
        meta.style.color = 'var(--text-muted)';
        meta.style.fontSize = '0.85rem';
        meta.textContent = (d.kind || '') + ' · ' + (d.status || '') + ' · ' + (d.created_at || '');
        delEl.append(job, sum, meta);
      } else {
        delEl.textContent = 'Belum ada delivery.';
      }
    }

    document.getElementById('menu-btn').addEventListener('click', toggleSidebar);
    document.getElementById('sidebar-backdrop').addEventListener('click', closeSidebar);
    document.addEventListener('keydown', (e) => { if (e.key === 'Escape') closeSidebar(); });
    // Any sidebar button closes the drawer on mobile (nav + session
    // rows): delegated, so the B1 session wiring stays untouched.
    document.querySelector('.sidebar').addEventListener('click', (e) => {
      if (isDrawerMode() && e.target.closest('button')) closeSidebar();
    });
    document.getElementById('ov-refresh-btn').addEventListener('click', loadOverview);"#;
