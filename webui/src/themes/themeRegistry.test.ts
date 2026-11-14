import test from 'node:test';
import assert from 'node:assert/strict';
import {
  COMPANION_PRESETS,
  THEME_PRESETS,
  deriveCompanionMood,
  getContrastRatio,
  resolveThemeTokens,
} from './themeRegistry';

test('all built-in theme presets pass WCAG AA text contrast between canvas and ink', () => {
  for (const preset of THEME_PRESETS) {
    const ratio = getContrastRatio(preset.tokens.canvas, preset.tokens.ink);
    assert.ok(ratio >= 4.5, `${preset.id} canvas/ink contrast (${ratio}) must be >= 4.5`);
    const codeRatio = getContrastRatio(preset.tokens.codeBg, preset.tokens.codeText);
    assert.ok(codeRatio >= 7, `${preset.id} codeBg/codeText contrast (${codeRatio}) must be >= 7`);
  }
});

test('resolveThemeTokens returns matching preset or merges custom overrides safely', () => {
  const midnight = resolveThemeTokens('tuxedo-midnight');
  assert.equal(midnight.canvas, '#141312');

  const custom = resolveThemeTokens('custom', {
    canvas: '#f0f0f0',
    primary: '#2255aa',
  });
  assert.equal(custom.canvas, '#f0f0f0');
  assert.equal(custom.primary, '#2255aa');
  assert.equal(custom.ink, THEME_PRESETS[0].tokens.ink);
});

test('deriveCompanionMood prioritizes streaming, then approval, then oyen preset', () => {
  assert.equal(
    deriveCompanionMood({ isStreaming: true, hasPendingApproval: true, activePreset: 'oyen' }),
    'thinking'
  );
  assert.equal(
    deriveCompanionMood({ isStreaming: false, hasPendingApproval: true, activePreset: 'oyen' }),
    'approval'
  );
  assert.equal(
    deriveCompanionMood({ isStreaming: false, hasPendingApproval: false, activePreset: 'oyen' }),
    'excited'
  );
  assert.equal(
    deriveCompanionMood({ isStreaming: false, hasPendingApproval: false, activePreset: 'tabby' }),
    'idle'
  );
});

test('companion presets include all 4 expected Clowder/Oyen archetypes', () => {
  const ids = COMPANION_PRESETS.map((p) => p.id);
  assert.deepEqual(ids, ['meowstro', 'tinker', 'whisker', 'oyen']);
});
