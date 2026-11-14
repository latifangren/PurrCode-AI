import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import assert from 'node:assert/strict';
import {
  AtomicStateStore,
  CURRENT_SCHEMA_VERSION,
  migrateCockpitState,
} from './store';

test('migrateCockpitState upgrades legacy v1 payloads and preserves custom sessions', () => {
  const legacyPayload = {
    activeSessionId: 'custom-99',
    sessions: [
      {
        id: 'custom-99',
        title: 'Legacy Session',
        branch: 'main',
        preset: 'carrier',
        model: 'gemini-4-hydrogen',
        updatedAt: '12:00:00',
        messages: [],
      },
    ],
    preferences: {
      activeThemeId: 'tuxedo-midnight',
    },
  };

  const { state, migrated } = migrateCockpitState(legacyPayload);
  assert.equal(migrated, true);
  assert.equal(state.schemaVersion, CURRENT_SCHEMA_VERSION);
  assert.equal(state.activeSessionId, 'custom-99');
  assert.equal(state.preferences.activeThemeId, 'tuxedo-midnight');
  assert.ok(state.preferences.companion.name.length > 0);
  assert.ok(state.providers.length > 0);
});

test('AtomicStateStore writes atomically and creates pre-migration backup', () => {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'purrcode-store-'));
  const statePath = path.join(tmpDir, 'state.json');

  // Seed legacy v1 state file without schemaVersion
  fs.writeFileSync(
    statePath,
    JSON.stringify({
      activeThemeId: 'warm-cream',
      sessions: [
        {
          id: 'legacy-1',
          title: 'Old Task',
          branch: 'dev',
          preset: 'tabby',
          model: 'gemini-4-hydrogen',
          updatedAt: '09:00:00',
          messages: [],
        },
      ],
    }),
    'utf-8'
  );

  const store = new AtomicStateStore(statePath);
  assert.equal(store.state.schemaVersion, CURRENT_SCHEMA_VERSION);
  assert.equal(store.state.sessions[0].id, 'legacy-1');
  assert.equal(fs.existsSync(`${statePath}.bak`), true);

  store.state.projectName = 'PurrCode-Hardened';
  store.persist();

  const reloaded = new AtomicStateStore(statePath);
  assert.equal(reloaded.state.projectName, 'PurrCode-Hardened');
  assert.equal(reloaded.getHealthSummary().status, 'ok');

  fs.rmSync(tmpDir, { recursive: true, force: true });
});
