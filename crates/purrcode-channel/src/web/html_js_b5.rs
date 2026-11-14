// SPDX-License-Identifier: GPL-3.0-or-later

//! WebUI JavaScript, part 2 (v0.4 B5): memory + skill management.
//! Concatenated after `WEB_INDEX_JS` into the same `<script>` by
//! `web_index_html`, so the two halves share top-level `let`
//! state and helpers (`apiUrl`, `escapeHtml`). Split out when
//! B5 pushed `html_js.rs` past the ADR-011 800-line limit.

pub const WEB_INDEX_JS_B5: &str = r#"    let currentMemoryFile = 'MEMORY.md';
    let editSkillName = '';
    let editSkillScope = 'project';
    let editSkillDescription = '';

    async function loadMemoryInfo() {
      try {
        const res = await fetch(apiUrl('/api/memory/info'));
        if (!res.ok) return;
        const data = await res.json();
        const facts = (data.files || []).map(f => f.name + ' ' + (f.exists ? f.bytes + ' B' : 'missing')).join(' · ');
        document.getElementById('memory-info').textContent = 'Backend: ' + (data.backend || 'markdown') + ' · ' + facts;
      } catch (e) {
        console.error('Failed to load memory info:', e);
      }
    }

    async function loadMemoryDoc(doc) {
      try {
        const res = await fetch(apiUrl(`/api/memory?file=${doc}`));
        if (!res.ok) return;
        const text = await res.text();
        currentMemoryFile = doc;
        document.getElementById('memory-content').textContent = text;
        document.getElementById('memory-editor').value = text;
        document.getElementById('memory-status').textContent = '';
      } catch (e) {
        console.error('Failed to load memory doc:', e);
      }
    }

    async function saveMemoryDoc() {
      const status = document.getElementById('memory-status');
      try {
        const res = await fetch(apiUrl('/api/memory'), {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ file: currentMemoryFile, content: document.getElementById('memory-editor').value })
        });
        const data = await res.json().catch(() => ({}));
        if (res.ok && data.saved) {
          status.textContent = `🐾 ${currentMemoryFile} saved (${data.bytes} B).`;
          loadMemoryDoc(currentMemoryFile);
        } else {
          status.textContent = 'Save failed: ' + (data.error || res.status);
        }
      } catch (e) {
        status.textContent = 'Save failed: ' + e;
      }
    }

    async function loadAllSkills() {
      const container = document.getElementById('skills-all-container');
      try {
        const res = await fetch(apiUrl('/api/skills/all'));
        if (!res.ok) {
          container.textContent = 'Failed to load skills.';
          return;
        }
        const data = await res.json();
        const skills = Array.isArray(data.skills) ? data.skills : [];
        container.innerHTML = '';
        if (skills.length === 0) {
          container.textContent = 'No skills found.';
          return;
        }
        for (const s of skills) {
          const card = document.createElement('div');
          card.className = 'view-card';
          card.style.display = 'flex';
          card.style.justifyContent = 'space-between';
          card.style.alignItems = 'center';
          const left = document.createElement('div');
          const name = document.createElement('strong');
          name.textContent = s.name || '';
          left.appendChild(name);
          const desc = document.createElement('div');
          desc.style.color = 'var(--text-muted)';
          desc.style.fontSize = '0.85rem';
          desc.textContent = (s.description || '') + ' · ' + (s.source || '') + (s.path ? ' · ' + s.path : '');
          left.appendChild(desc);
          card.appendChild(left);
          const actions = document.createElement('div');
          actions.style.display = 'flex';
          actions.style.gap = '8px';
          const editable = s.source !== 'embedded';
          const viewBtn = document.createElement('button');
          viewBtn.className = 'btn btn-secondary';
          viewBtn.textContent = editable ? 'Edit' : 'View';
          viewBtn.dataset.skillName = s.name || '';
          viewBtn.addEventListener('click', () => openSkillEditor(viewBtn.dataset.skillName, editable));
          actions.appendChild(viewBtn);
          if (editable) {
            const delBtn = document.createElement('button');
            delBtn.className = 'btn btn-danger';
            delBtn.textContent = 'Delete';
            delBtn.dataset.skillName = s.name || '';
            delBtn.dataset.skillSource = s.source || '';
            delBtn.addEventListener('click', () => deleteSkill(delBtn.dataset.skillName, delBtn.dataset.skillSource));
            actions.appendChild(delBtn);
          }
          card.appendChild(actions);
          container.appendChild(card);
        }
      } catch (e) {
        console.error('Failed to load skills:', e);
      }
    }

    async function openSkillEditor(name, editable) {
      try {
        const res = await fetch(apiUrl('/api/skills/' + encodeURIComponent(name)));
        if (!res.ok) return;
        const data = await res.json();
        const skill = data.skill || {};
        editSkillName = skill.name || name;
        editSkillScope = skill.source === 'global' ? 'global' : 'project';
        editSkillDescription = skill.description || '';
        document.getElementById('skill-edit-name').value = editSkillName;
        document.getElementById('skill-edit-scope').textContent = '(' + editSkillScope + ')';
        document.getElementById('skill-edit-content').value = skill.content || '';
        document.getElementById('skill-edit-content').disabled = !editable;
        document.getElementById('skill-edit-save').style.display = editable ? '' : 'none';
        document.getElementById('skill-edit-card').style.display = 'block';
        document.getElementById('skill-edit-card').scrollIntoView();
      } catch (e) {
        console.error('Failed to load skill:', e);
      }
    }

    async function createSkill() {
      const status = document.getElementById('skills-status');
      const name = document.getElementById('skill-add-name').value.trim();
      const description = document.getElementById('skill-add-description').value.trim();
      const content = document.getElementById('skill-add-content').value;
      const scope = document.getElementById('skill-add-scope').value;
      if (!name || !description || !content) {
        status.textContent = 'Name, description, and content are all required.';
        return;
      }
      try {
        const res = await fetch(apiUrl('/api/skills/create'), {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ name, description, content, scope })
        });
        const data = await res.json().catch(() => ({}));
        if (res.ok && data.created) {
          status.textContent = `🐾 Skill '${name}' created (${scope}).`;
          document.getElementById('skill-add-name').value = '';
          document.getElementById('skill-add-description').value = '';
          document.getElementById('skill-add-content').value = '';
          loadAllSkills();
        } else {
          status.textContent = 'Create failed: ' + (data.error || res.status);
        }
      } catch (e) {
        status.textContent = 'Create failed: ' + e;
      }
    }

    async function updateSkill() {
      const status = document.getElementById('skills-status');
      try {
        const res = await fetch(apiUrl('/api/skills/update'), {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ name: editSkillName, description: editSkillDescription, content: document.getElementById('skill-edit-content').value, scope: editSkillScope })
        });
        const data = await res.json().catch(() => ({}));
        if (res.ok && data.updated) {
          status.textContent = `🐾 Skill '${editSkillName}' updated.`;
          document.getElementById('skill-edit-card').style.display = 'none';
          loadAllSkills();
        } else {
          status.textContent = 'Update failed: ' + (data.error || res.status);
        }
      } catch (e) {
        status.textContent = 'Update failed: ' + e;
      }
    }

    async function deleteSkill(name, source) {
      if (!confirm(`Delete skill '${name}' (${source})? This cannot be undone.`)) return;
      const status = document.getElementById('skills-status');
      try {
        const res = await fetch(apiUrl('/api/skills/delete'), {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ name, scope: source })
        });
        const data = await res.json().catch(() => ({}));
        if (res.ok && data.deleted) {
          status.textContent = `🐾 Skill '${name}' deleted.`;
          loadAllSkills();
        } else {
          status.textContent = 'Delete failed: ' + (data.error || res.status);
        }
      } catch (e) {
        status.textContent = 'Delete failed: ' + e;
      }
    }
"#;
