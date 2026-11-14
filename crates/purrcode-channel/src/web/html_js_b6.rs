// SPDX-License-Identifier: GPL-3.0-or-later

//! WebUI JavaScript, part 3 (v0.4 B6): cron scheduler management.
//! Concatenated after `WEB_INDEX_JS` and `WEB_INDEX_JS_B5` into
//! the same `<script>` by `web_index_html`, so the three parts
//! share top-level `let` state and helpers (`apiUrl`,
//! `escapeHtml`, `loadCronJobs`). Split out the way B5 did: the
//! cron tab's add form, parse preview, and enable/disable/remove
//! actions live here, while the table rendering stays in
//! `loadCronJobs` (`html_js.rs`).
//!
//! Action buttons carry no inline handlers (the B3 tripwire
//! forbids `onclick=` in the served JS — job ids come from the
//! store and must never be interpolated into a JS string
//! literal): each button has `data-cron-id` + `data-cron-action`
//! and one delegated listener on `#cron-tbody` dispatches them.

pub const WEB_INDEX_JS_B6: &str = r#"    async function cronParsePreview() {
      const schedule = document.getElementById('cron-add-schedule').value;
      const preview = document.getElementById('cron-parse-preview');
      preview.textContent = '';
      if (!schedule.trim()) { preview.textContent = 'Enter a schedule first.'; return; }
      try {
        const res = await fetch(apiUrl('/api/cron/parse'), {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ schedule })
        });
        const data = await res.json();
        if (!res.ok) { preview.textContent = 'Could not parse: ' + (data.error || res.status); return; }
        const s = data.schedule || {};
        preview.textContent = 'Understood as: ' + (s.display || '') + ' — next run ' + (s.next_run || '—') + (s.once ? ' (one-shot)' : '');
      } catch (e) {
        preview.textContent = 'Parse failed: ' + e;
      }
    }

    async function cronCreateJob() {
      const status = document.getElementById('cron-add-status');
      status.textContent = '';
      const name = document.getElementById('cron-add-name').value.trim();
      const schedule = document.getElementById('cron-add-schedule').value;
      const prompt = document.getElementById('cron-add-prompt').value;
      const allowRaw = document.getElementById('cron-add-allow').value.trim();
      const preset = document.getElementById('cron-add-preset').value.trim();
      const body = { schedule, prompt };
      if (name) body.name = name;
      if (allowRaw) body.allow = allowRaw.split(',').map(t => t.trim()).filter(t => t);
      if (preset) body.preset = preset;
      try {
        const res = await fetch(apiUrl('/api/cron/add'), {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body)
        });
        const data = await res.json();
        if (!res.ok) { status.textContent = 'Could not create job: ' + (data.error || res.status); return; }
        const job = data.job || {};
        status.textContent = 'Job created: ' + (job.name || '') + ' — next run ' + (job.next_run || '—');
        document.getElementById('cron-add-name').value = '';
        document.getElementById('cron-add-schedule').value = '';
        document.getElementById('cron-add-prompt').value = '';
        document.getElementById('cron-add-allow').value = '';
        document.getElementById('cron-add-preset').value = '';
        document.getElementById('cron-parse-preview').textContent = '';
        loadCronJobs();
      } catch (e) {
        status.textContent = 'Create failed: ' + e;
      }
    }

    async function cronToggleJob(id, action) {
      const path = action === 'enable' ? '/api/cron/enable' : '/api/cron/disable';
      try {
        const res = await fetch(apiUrl(path), {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ id })
        });
        if (!res.ok) {
          const data = await res.json().catch(() => ({}));
          console.error('Cron ' + action + ' failed:', data.error || res.status);
          return;
        }
        loadCronJobs();
      } catch (e) {
        console.error('Cron ' + action + ' failed:', e);
      }
    }

    async function cronRemoveJob(id) {
      if (!confirm('Remove this cron job?')) return;
      try {
        const res = await fetch(apiUrl('/api/cron/remove'), {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ id })
        });
        if (!res.ok) {
          const data = await res.json().catch(() => ({}));
          console.error('Cron remove failed:', data.error || res.status);
          return;
        }
        loadCronJobs();
      } catch (e) {
        console.error('Cron remove failed:', e);
      }
    }

    document.getElementById('cron-parse-btn').addEventListener('click', cronParsePreview);
    document.getElementById('cron-add-btn').addEventListener('click', cronCreateJob);
    document.getElementById('cron-tbody').addEventListener('click', (event) => {
      const btn = event.target.closest('[data-cron-action]');
      if (!btn) return;
      const id = btn.dataset.cronId;
      const action = btn.dataset.cronAction;
      if (!id) return;
      if (action === 'remove') cronRemoveJob(id);
      else if (action === 'enable' || action === 'disable') cronToggleJob(id, action);
    });
"#;
