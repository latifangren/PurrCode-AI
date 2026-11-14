import React, { useEffect, useState } from 'react';
import { Check, Copy, RotateCcw } from 'lucide-react';
import {
  COMPANION_PRESETS,
  DEFAULT_PREFERENCES,
  THEME_PRESETS,
  getContrastRatio,
  resolveThemeTokens,
} from '../themes/themeRegistry';
import type {
  CockpitPreferences,
  CompanionAccessory,
  CompanionMood,
  CompanionPattern,
  CompanionVisibility,
  ThemeTokens,
} from '../types/cockpit';
import { CatCompanionSvg } from './CatCompanionSvg';

interface AppearanceViewProps {
  preferences: CockpitPreferences;
  onUpdatePreferences: (next: CockpitPreferences) => Promise<void>;
}

const CUSTOM_COLOR_FIELDS: { key: keyof ThemeTokens; label: string }[] = [
  { key: 'canvas', label: 'Canvas Background' },
  { key: 'surfaceCard', label: 'Card Surface' },
  { key: 'ink', label: 'Primary Ink Text' },
  { key: 'primary', label: 'Brand Accent CTA' },
  { key: 'codeBg', label: 'Code & Diff Window' },
];

export function AppearanceView({
  preferences,
  onUpdatePreferences,
}: AppearanceViewProps) {
  const [previewMood, setPreviewMood] = useState<CompanionMood>('idle');
  const [savingMsg, setSavingMsg] = useState(false);

  const activeTokens = resolveThemeTokens(
    preferences.activeThemeId,
    preferences.customThemeTokens
  );
  const contrastRatio = getContrastRatio(activeTokens.canvas, activeTokens.ink);
  const contrastPass = contrastRatio >= 4.5;

  const commit = async (next: CockpitPreferences) => {
    setSavingMsg(true);
    await onUpdatePreferences(next);
    setTimeout(() => setSavingMsg(false), 1200);
  };

  const selectThemePreset = (presetId: string) => {
    const found = THEME_PRESETS.find((t) => t.id === presetId);
    if (!found) return;
    commit({
      ...preferences,
      activeThemeId: presetId,
    });
  };

  const duplicateToCustom = () => {
    commit({
      ...preferences,
      activeThemeId: 'custom',
      customThemeTokens: { ...activeTokens },
    });
  };

  const updateCustomToken = (key: keyof ThemeTokens, value: string) => {
    const nextTokens: ThemeTokens = {
      ...preferences.customThemeTokens,
      [key]: value,
    };
    if (key === 'primary') {
      nextTokens.primaryActive = value;
    }
    commit({
      ...preferences,
      activeThemeId: 'custom',
      customThemeTokens: nextTokens,
    });
  };

  const selectCompanionPreset = (presetId: string) => {
    const found = COMPANION_PRESETS.find((p) => p.id === presetId);
    if (!found) return;
    commit({
      ...preferences,
      companion: {
        ...found.config,
        visibility: preferences.companion.visibility,
      },
    });
  };

  const updateCompanion = (patch: Partial<CockpitPreferences['companion']>) => {
    commit({
      ...preferences,
      companion: {
        ...preferences.companion,
        ...patch,
      },
    });
  };

  return (
    <div className="flex-1 overflow-y-auto px-4 sm:px-8 py-8">
      <div className="max-w-5xl mx-auto space-y-10">
        <header className="flex flex-wrap items-end justify-between gap-4 pb-5 border-b border-hairline">
          <div className="space-y-1.5">
            <h1 className="font-editorial text-4xl text-ink">
              Themes & Cat Companion Studio
            </h1>
            <p className="text-sm text-body max-w-2xl leading-relaxed">
              Switch modular color palettes, customize semantic CSS tokens with live contrast verification, and personalize your PurrCode feline companion.
            </p>
          </div>
          {savingMsg && (
            <span className="inline-flex items-center gap-1.5 text-xs font-medium text-success">
              <Check className="w-3.5 h-3.5" />
              <span>Preferences Saved</span>
            </span>
          )}
        </header>

        {/* SECTION 1: Modular Color Themes */}
        <section className="space-y-5">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <h2 className="font-editorial text-2xl text-ink">Modular Theme Presets</h2>
              <p className="text-xs text-muted">
                Every surface, border, button, and code block binds dynamically to CSS variables.
              </p>
            </div>
            <button
              type="button"
              onClick={duplicateToCustom}
              className="h-9 px-3.5 inline-flex items-center gap-1.5 rounded-md border border-hairline bg-canvas hover:bg-surface-card text-xs font-medium text-ink transition-colors cursor-pointer"
            >
              <Copy className="w-3.5 h-3.5 text-primary" />
              <span>Customize Active Theme</span>
            </button>
          </div>

          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
            {THEME_PRESETS.map((preset) => {
              const isSelected = preferences.activeThemeId === preset.id;
              return (
                <button
                  key={preset.id}
                  type="button"
                  onClick={() => selectThemePreset(preset.id)}
                  className={`rounded-xl border p-4 text-left space-y-3 transition-colors cursor-pointer ${
                    isSelected
                      ? 'border-primary bg-surface-card'
                      : 'border-hairline bg-surface-soft hover:bg-surface-card'
                  }`}
                >
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-sm font-medium text-ink">{preset.name}</span>
                    {isSelected && <Check className="w-4 h-4 text-primary shrink-0" />}
                  </div>

                  {/* Swatch Strip */}
                  <div className="flex items-center gap-1.5">
                    <span
                      className="w-6 h-6 rounded border border-hairline"
                      style={{ backgroundColor: preset.tokens.canvas }}
                      title="Canvas"
                    />
                    <span
                      className="w-6 h-6 rounded border border-hairline"
                      style={{ backgroundColor: preset.tokens.surfaceCard }}
                      title="Card"
                    />
                    <span
                      className="w-6 h-6 rounded"
                      style={{ backgroundColor: preset.tokens.primary }}
                      title="Primary"
                    />
                    <span
                      className="w-6 h-6 rounded"
                      style={{ backgroundColor: preset.tokens.codeBg }}
                      title="Code Window"
                    />
                  </div>

                  <p className="text-xs text-muted leading-relaxed">{preset.description}</p>
                </button>
              );
            })}
          </div>

          {/* Custom Theme Token Editor */}
          <div className="rounded-xl border border-hairline bg-surface-soft p-6 space-y-4">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div>
                <h3 className="text-sm font-medium text-ink">
                  Custom Theme Token Overrides{' '}
                  {preferences.activeThemeId === 'custom' && (
                    <span className="text-primary ml-1">· Active</span>
                  )}
                </h3>
                <p className="text-xs text-muted">
                  Adjust core palette tokens directly. Contrast is checked against WCAG AA (4.5:1).
                </p>
              </div>
              <div className="flex flex-wrap items-center gap-3">
                <span
                  className={`font-mono text-xs tabular-nums ${
                    contrastPass ? 'text-success' : 'text-error font-medium'
                  }`}
                >
                  Canvas/Ink Contrast: {contrastRatio}:1 ({contrastPass ? 'AA Pass' : 'Low Contrast'})
                </span>
                <button
                  type="button"
                  onClick={() => selectThemePreset('warm-cream')}
                  className="h-8 px-2.5 inline-flex items-center gap-1 rounded border border-hairline bg-canvas text-xs text-body hover:text-ink cursor-pointer"
                >
                  <RotateCcw className="w-3 h-3" />
                  <span>Reset</span>
                </button>
              </div>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-5 gap-3">
              {CUSTOM_COLOR_FIELDS.map(({ key, label }) => (
                <HexTokenField
                  key={key}
                  label={label}
                  value={activeTokens[key]}
                  onCommit={(hex) => updateCustomToken(key, hex)}
                />
              ))}
            </div>
          </div>
        </section>

        {/* SECTION 2: Cat Companion Presets & Customizer */}
        <section className="space-y-5 pt-4 border-t border-hairline">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <h2 className="font-editorial text-2xl text-ink">
                Cat Companion & Live Animations
              </h2>
              <p className="text-xs text-muted">
                Choose a Clowder feline preset or craft your own companion to react during thinking, tool calls, and approvals.
              </p>
            </div>

            <div className="inline-flex rounded-md border border-hairline bg-surface-soft p-0.5 text-xs">
              {(['full', 'compact', 'hidden'] as CompanionVisibility[]).map((vis) => (
                <button
                  key={vis}
                  type="button"
                  onClick={() => updateCompanion({ visibility: vis })}
                  className={`px-3 py-1 rounded font-medium capitalize transition-colors cursor-pointer ${
                    preferences.companion.visibility === vis
                      ? 'bg-canvas text-ink'
                      : 'text-muted hover:text-ink'
                  }`}
                >
                  {vis}
                </button>
              ))}
            </div>
          </div>

          <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
            {/* Interactive Live Stage */}
            <div className="rounded-xl border border-hairline bg-surface-card p-6 flex flex-col items-center justify-between gap-5 text-center">
              <div className="space-y-1">
                <div className="font-editorial text-2xl text-ink">
                  {preferences.companion.name}
                </div>
                <p className="text-xs text-muted">{preferences.companion.tagline}</p>
              </div>

              <div className="py-2">
                <CatCompanionSvg
                  companion={preferences.companion}
                  mood={previewMood}
                  size="lg"
                />
              </div>

              <div className="w-full space-y-2">
                <div className="text-[11px] uppercase tracking-[0.08em] text-muted font-medium">
                  Preview Animation State
                </div>
                <div className="grid grid-cols-2 gap-1.5">
                  {(['idle', 'thinking', 'approval', 'excited'] as CompanionMood[]).map((m) => (
                    <button
                      key={m}
                      type="button"
                      onClick={() => setPreviewMood(m)}
                      className={`h-8 rounded-md text-xs font-medium capitalize transition-colors cursor-pointer ${
                        previewMood === m
                          ? 'bg-primary text-on-primary'
                          : 'border border-hairline bg-canvas text-body hover:text-ink'
                      }`}
                    >
                      {m}
                    </button>
                  ))}
                </div>
              </div>
            </div>

            {/* Presets + Customizer Controls */}
            <div className="lg:col-span-2 rounded-xl border border-hairline bg-canvas p-6 space-y-6">
              <div className="space-y-2.5">
                <div className="text-xs font-medium text-body-strong">Companion Presets</div>
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-2.5">
                  {COMPANION_PRESETS.map((preset) => {
                    const active = preferences.companion.presetId === preset.id;
                    return (
                      <button
                        key={preset.id}
                        type="button"
                        onClick={() => selectCompanionPreset(preset.id)}
                        className={`flex items-center gap-3 rounded-lg border p-3 text-left transition-colors cursor-pointer ${
                          active
                            ? 'border-primary bg-surface-card'
                            : 'border-hairline bg-surface-soft hover:bg-surface-card'
                        }`}
                      >
                        <CatCompanionSvg companion={preset.config} mood="idle" size="sm" />
                        <div className="min-w-0">
                          <div className="text-sm font-medium text-ink">{preset.config.name}</div>
                          <div className="text-[11px] text-muted truncate">{preset.roleHint}</div>
                        </div>
                      </button>
                    );
                  })}
                </div>
              </div>

              <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 pt-4 border-t border-hairline">
                <label className="space-y-1.5 text-xs font-medium text-body">
                  <span>Companion Name</span>
                  <input
                    type="text"
                    value={preferences.companion.name}
                    onChange={(e) =>
                      updateCompanion({ presetId: 'custom', name: e.target.value })
                    }
                    className="h-9 w-full rounded-md border border-hairline bg-canvas px-3 text-sm text-ink focus:outline-none focus:border-primary"
                  />
                </label>

                <label className="space-y-1.5 text-xs font-medium text-body">
                  <span>Fur Pattern</span>
                  <select
                    value={preferences.companion.pattern}
                    onChange={(e) =>
                      updateCompanion({
                        presetId: 'custom',
                        pattern: e.target.value as CompanionPattern,
                      })
                    }
                    className="h-9 w-full rounded-md border border-hairline bg-canvas px-3 text-sm text-ink focus:outline-none focus:border-primary"
                  >
                    <option value="tuxedo">Tuxedo</option>
                    <option value="tabby">Tabby Stripes</option>
                    <option value="calico">Calico Patches</option>
                    <option value="siamese">Siamese Point</option>
                    <option value="solid">Solid Coat</option>
                  </select>
                </label>

                <label className="space-y-1.5 text-xs font-medium text-body">
                  <span>Accessory</span>
                  <select
                    value={preferences.companion.accessory}
                    onChange={(e) =>
                      updateCompanion({
                        presetId: 'custom',
                        accessory: e.target.value as CompanionAccessory,
                      })
                    }
                    className="h-9 w-full rounded-md border border-hairline bg-canvas px-3 text-sm text-ink focus:outline-none focus:border-primary"
                  >
                    <option value="none">None</option>
                    <option value="glasses">Developer Glasses</option>
                    <option value="headphones">Studio Headphones</option>
                    <option value="scarf">Accent Scarf</option>
                  </select>
                </label>

                <div className="grid grid-cols-3 gap-2">
                  <label className="space-y-1.5 text-xs font-medium text-body">
                    <span>Coat 1</span>
                    <input
                      type="color"
                      aria-label="Primary fur color"
                      value={preferences.companion.furPrimary}
                      onChange={(e) =>
                        updateCompanion({ presetId: 'custom', furPrimary: e.target.value })
                      }
                      className="h-9 w-full rounded border border-hairline bg-transparent cursor-pointer"
                    />
                  </label>
                  <label className="space-y-1.5 text-xs font-medium text-body">
                    <span>Coat 2</span>
                    <input
                      type="color"
                      aria-label="Secondary fur color"
                      value={preferences.companion.furSecondary}
                      onChange={(e) =>
                        updateCompanion({ presetId: 'custom', furSecondary: e.target.value })
                      }
                      className="h-9 w-full rounded border border-hairline bg-transparent cursor-pointer"
                    />
                  </label>
                  <label className="space-y-1.5 text-xs font-medium text-body">
                    <span>Eyes</span>
                    <input
                      type="color"
                      aria-label="Eye color"
                      value={preferences.companion.eyeColor}
                      onChange={(e) =>
                        updateCompanion({ presetId: 'custom', eyeColor: e.target.value })
                      }
                      className="h-9 w-full rounded border border-hairline bg-transparent cursor-pointer"
                    />
                  </label>
                </div>

                <label className="sm:col-span-2 space-y-1.5 text-xs font-medium text-body">
                  <span>Companion Tagline</span>
                  <input
                    type="text"
                    value={preferences.companion.tagline}
                    onChange={(e) =>
                      updateCompanion({ presetId: 'custom', tagline: e.target.value })
                    }
                    className="h-9 w-full rounded-md border border-hairline bg-canvas px-3 text-sm text-ink focus:outline-none focus:border-primary"
                  />
                </label>
              </div>
            </div>
          </div>
        </section>
      </div>
    </div>
  );
}

function HexTokenField({
  label,
  value,
  onCommit,
}: {
  label: string;
  value: string;
  onCommit: (hex: string) => void;
}) {
  const [draft, setDraft] = useState(value);

  useEffect(() => {
    setDraft(value);
  }, [value]);

  return (
    <label className="rounded-lg border border-hairline bg-canvas p-3 space-y-2 text-xs">
      <span className="block font-medium text-body-strong">{label}</span>
      <div className="flex items-center gap-2">
        <input
          type="color"
          aria-label={`${label} picker`}
          value={value}
          onChange={(e) => onCommit(e.target.value)}
          className="w-8 h-8 rounded border border-hairline bg-transparent cursor-pointer shrink-0"
        />
        <input
          type="text"
          aria-label={`${label} hex`}
          value={draft}
          onChange={(e) => {
            const next = e.target.value;
            setDraft(next);
            if (/^#[0-9a-fA-F]{6}$/.test(next)) {
              onCommit(next);
            }
          }}
          onBlur={() => {
            if (!/^#[0-9a-fA-F]{6}$/.test(draft)) {
              setDraft(value);
            }
          }}
          className="w-full min-w-0 font-mono text-xs text-ink bg-transparent focus:outline-none"
        />
      </div>
    </label>
  );
}
