import type {
  CockpitPreferences,
  CompanionConfig,
  CompanionMood,
  PresetMode,
  ThemePreset,
  ThemeTokens,
} from '../types/cockpit';

export const THEME_PRESETS: ThemePreset[] = [
  {
    id: 'warm-cream',
    name: 'Warm Editorial Cream',
    description: 'Tinted cream canvas with warm coral actions and dark code chrome.',
    isDark: false,
    tokens: {
      canvas: '#faf9f5',
      surfaceSoft: '#f5f0e8',
      surfaceCard: '#efe9de',
      surfaceStrong: '#e8e0d2',
      ink: '#141413',
      body: '#3d3d3a',
      bodyStrong: '#252523',
      muted: '#6c6a64',
      mutedSoft: '#8e8b82',
      hairline: '#e6dfd8',
      hairlineSoft: '#ebe6df',
      primary: '#cc785c',
      primaryActive: '#a9583e',
      primaryDisabled: '#e6dfd8',
      onPrimary: '#ffffff',
      codeBg: '#181715',
      codeSoft: '#1f1e1b',
      codeElevated: '#252320',
      codeBorder: '#2d2b27',
      codeText: '#faf9f5',
      codeMuted: '#a09d96',
      accentTeal: '#5db8a6',
      accentAmber: '#e8a55a',
      success: '#3d7a4a',
      warning: '#d4a017',
      error: '#c64545',
    },
  },
  {
    id: 'tuxedo-midnight',
    name: 'Tuxedo Midnight',
    description: 'Warm obsidian surface with sand typography and muted terracotta.',
    isDark: true,
    tokens: {
      canvas: '#141312',
      surfaceSoft: '#1b1917',
      surfaceCard: '#22201d',
      surfaceStrong: '#2c2925',
      ink: '#f3ede4',
      body: '#d4cdc2',
      bodyStrong: '#e8e2d8',
      muted: '#9e978d',
      mutedSoft: '#7b746b',
      hairline: '#2e2b27',
      hairlineSoft: '#25231f',
      primary: '#d98468',
      primaryActive: '#cc785c',
      primaryDisabled: '#332f2b',
      onPrimary: '#141312',
      codeBg: '#0d0c0b',
      codeSoft: '#151412',
      codeElevated: '#1f1d1a',
      codeBorder: '#2c2925',
      codeText: '#f3ede4',
      codeMuted: '#9e978d',
      accentTeal: '#64c2af',
      accentAmber: '#ebb067',
      success: '#6bc47e',
      warning: '#e0b034',
      error: '#e06666',
    },
  },
  {
    id: 'matcha-siamese',
    name: 'Matcha Siamese',
    description: 'Soft botanical sage canvas paired with deep forest ink and jade.',
    isDark: false,
    tokens: {
      canvas: '#f4f6f1',
      surfaceSoft: '#ebf0e6',
      surfaceCard: '#e2e9dc',
      surfaceStrong: '#d5dfd0',
      ink: '#162019',
      body: '#314036',
      bodyStrong: '#1e2c23',
      muted: '#5f6f64',
      mutedSoft: '#7e8e83',
      hairline: '#d7e0d2',
      hairlineSoft: '#e3eae0',
      primary: '#4f8a6e',
      primaryActive: '#3c6e56',
      primaryDisabled: '#d7e0d2',
      onPrimary: '#ffffff',
      codeBg: '#131b16',
      codeSoft: '#19241d',
      codeElevated: '#212e26',
      codeBorder: '#2c3b32',
      codeText: '#f4f6f1',
      codeMuted: '#9aaea0',
      accentTeal: '#5db8a6',
      accentAmber: '#d99b4e',
      success: '#3b7a4b',
      warning: '#c9931a',
      error: '#c44b4b',
    },
  },
  {
    id: 'oyen-amber',
    name: 'Oyen Amber',
    description: 'Sun-warmed paper canvas with espresso ink and ginger amber accent.',
    isDark: false,
    tokens: {
      canvas: '#fbf6ee',
      surfaceSoft: '#f4ecdf',
      surfaceCard: '#ede1ce',
      surfaceStrong: '#e3d3bb',
      ink: '#1f1712',
      body: '#473930',
      bodyStrong: '#2c211a',
      muted: '#77675c',
      mutedSoft: '#96867a',
      hairline: '#e6d8c5',
      hairlineSoft: '#eee4d5',
      primary: '#d47422',
      primaryActive: '#b35c14',
      primaryDisabled: '#e6d8c5',
      onPrimary: '#ffffff',
      codeBg: '#1b1410',
      codeSoft: '#241b16',
      codeElevated: '#2e231c',
      codeBorder: '#3a2d25',
      codeText: '#fbf6ee',
      codeMuted: '#ad9d91',
      accentTeal: '#4c9c8c',
      accentAmber: '#e59a3c',
      success: '#467c46',
      warning: '#c98a16',
      error: '#c44545',
    },
  },
];

export interface CompanionPreset {
  id: string;
  config: CompanionConfig;
  roleHint: string;
}

export const COMPANION_PRESETS: CompanionPreset[] = [
  {
    id: 'meowstro',
    roleHint: 'Lead Orchestrator · Tuxedo Strategist',
    config: {
      presetId: 'meowstro',
      name: 'Meowstro',
      tagline: 'Keeping module boundaries clean and fail-closed.',
      pattern: 'tuxedo',
      furPrimary: '#242220',
      furSecondary: '#faf6ef',
      eyeColor: '#e8a55a',
      accessory: 'none',
      visibility: 'full',
    },
  },
  {
    id: 'tinker',
    roleHint: 'Patch Fixer · Ginger Builder',
    config: {
      presetId: 'tinker',
      name: 'Tinker',
      tagline: 'Synthesizing surgical diffs under 800 lines.',
      pattern: 'tabby',
      furPrimary: '#cc785c',
      furSecondary: '#f7ead9',
      eyeColor: '#5db8a6',
      accessory: 'glasses',
      visibility: 'full',
    },
  },
  {
    id: 'whisker',
    roleHint: 'Codebase Scout · Siamese Explorer',
    config: {
      presetId: 'whisker',
      name: 'Whisker',
      tagline: 'Tracing AST paths and verifying Landlock rules.',
      pattern: 'siamese',
      furPrimary: '#ede3d4',
      furSecondary: '#5a4638',
      eyeColor: '#5db8a6',
      accessory: 'headphones',
      visibility: 'full',
    },
  },
  {
    id: 'oyen',
    roleHint: 'Autonomous Runner · High-Energy Ginger',
    config: {
      presetId: 'oyen',
      name: 'Oyen',
      tagline: 'Ready to ship at full speed.',
      pattern: 'calico',
      furPrimary: '#e0843c',
      furSecondary: '#faf5ec',
      eyeColor: '#4f8a6e',
      accessory: 'scarf',
      visibility: 'full',
    },
  },
];

export const DEFAULT_PREFERENCES: CockpitPreferences = {
  activeThemeId: 'warm-cream',
  customThemeTokens: { ...THEME_PRESETS[0].tokens },
  companion: { ...COMPANION_PRESETS[0].config },
};

export function resolveThemeTokens(
  activeThemeId: string,
  customTokens?: Partial<ThemeTokens>
): ThemeTokens {
  if (activeThemeId === 'custom' && customTokens) {
    return {
      ...THEME_PRESETS[0].tokens,
      ...customTokens,
    };
  }
  const preset = THEME_PRESETS.find((t) => t.id === activeThemeId) ?? THEME_PRESETS[0];
  return preset.tokens;
}

export function applyThemeToDocument(tokens: ThemeTokens) {
  if (typeof document === 'undefined') return;
  const root = document.documentElement.style;
  root.setProperty('--pc-canvas', tokens.canvas);
  root.setProperty('--pc-surface-soft', tokens.surfaceSoft);
  root.setProperty('--pc-surface-card', tokens.surfaceCard);
  root.setProperty('--pc-surface-strong', tokens.surfaceStrong);
  root.setProperty('--pc-ink', tokens.ink);
  root.setProperty('--pc-body', tokens.body);
  root.setProperty('--pc-body-strong', tokens.bodyStrong);
  root.setProperty('--pc-muted', tokens.muted);
  root.setProperty('--pc-muted-soft', tokens.mutedSoft);
  root.setProperty('--pc-hairline', tokens.hairline);
  root.setProperty('--pc-hairline-soft', tokens.hairlineSoft);
  root.setProperty('--pc-primary', tokens.primary);
  root.setProperty('--pc-primary-active', tokens.primaryActive);
  root.setProperty('--pc-primary-disabled', tokens.primaryDisabled);
  root.setProperty('--pc-on-primary', tokens.onPrimary);
  root.setProperty('--pc-code-bg', tokens.codeBg);
  root.setProperty('--pc-code-soft', tokens.codeSoft);
  root.setProperty('--pc-code-elevated', tokens.codeElevated);
  root.setProperty('--pc-code-border', tokens.codeBorder);
  root.setProperty('--pc-code-text', tokens.codeText);
  root.setProperty('--pc-code-muted', tokens.codeMuted);
  root.setProperty('--pc-accent-teal', tokens.accentTeal);
  root.setProperty('--pc-accent-amber', tokens.accentAmber);
  root.setProperty('--pc-success', tokens.success);
  root.setProperty('--pc-warning', tokens.warning);
  root.setProperty('--pc-error', tokens.error);
}

function hexToRgb(hex: string): [number, number, number] | null {
  const clean = hex.trim().replace('#', '');
  if (!/^[0-9a-fA-F]{6}$/.test(clean)) return null;
  const n = parseInt(clean, 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function relativeLuminance(rgb: [number, number, number]): number {
  const [r, g, b] = rgb.map((c) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

export function getContrastRatio(hexA: string, hexB: string): number {
  const rgbA = hexToRgb(hexA);
  const rgbB = hexToRgb(hexB);
  if (!rgbA || !rgbB) return 1;
  const l1 = relativeLuminance(rgbA);
  const l2 = relativeLuminance(rgbB);
  const bright = Math.max(l1, l2);
  const dark = Math.min(l1, l2);
  return Number(((bright + 0.05) / (dark + 0.05)).toFixed(2));
}

export function deriveCompanionMood(params: {
  isStreaming: boolean;
  hasPendingApproval: boolean;
  activePreset: PresetMode;
}): CompanionMood {
  if (params.isStreaming) return 'thinking';
  if (params.hasPendingApproval) return 'approval';
  if (params.activePreset === 'oyen') return 'excited';
  return 'idle';
}
