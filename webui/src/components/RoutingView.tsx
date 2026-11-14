import React, { useEffect, useMemo, useState } from 'react';
import {
  AlertTriangle,
  Check,
  Info,
  RotateCcw,
  Save,
  Server,
  SlidersHorizontal,
  Sparkles,
  Zap,
} from 'lucide-react';
import type {
  ProviderConfig,
  ReasoningEffort,
  SubagentRouting,
} from '../types/cockpit';

interface RoutingViewProps {
  routing: SubagentRouting[];
  providers?: ProviderConfig[];
  availableModels: string[];
  activeSessionModel?: string;
  onSaveRouting: (next: SubagentRouting[]) => Promise<void>;
  onOpenProviders?: () => void;
}

const EFFORT_OPTIONS: { value: ReasoningEffort; label: string }[] = [
  { value: 'auto', label: 'Auto (model default)' },
  { value: 'off', label: 'Off (fast / no thinking)' },
  { value: 'low', label: 'Low' },
  { value: 'medium', label: 'Medium' },
  { value: 'high', label: 'High (deep reasoning)' },
];

function normalizeEffort(raw: unknown): ReasoningEffort {
  if (typeof raw === 'string') {
    const low = raw.toLowerCase();
    if (low === 'none' || low === 'off') return 'off';
    if (low === 'low' || low === 'medium' || low === 'high' || low === 'auto') {
      return low;
    }
  }
  return 'auto';
}

function defaultEffortForRole(role: string): ReasoningEffort {
  switch (role) {
    case 'Tinker':
    case 'Sphinx':
    case 'Tuxeshadow':
      return 'high';
    case 'Whisker':
      return 'low';
    case 'Calico':
      return 'medium';
    default:
      return 'auto';
  }
}

function normalizeRows(rows: SubagentRouting[]): SubagentRouting[] {
  return rows.map((row) => ({
    role: row.role,
    responsibility: row.responsibility ?? '',
    primaryModel: row.primaryModel ?? '',
    fallbackModel: row.fallbackModel ?? '',
    reasoningEffort: row.reasoningEffort
      ? normalizeEffort(row.reasoningEffort)
      : defaultEffortForRole(row.role),
    temperature:
      typeof row.temperature === 'number' && Number.isFinite(row.temperature)
        ? row.temperature
        : null,
    maxTokens:
      typeof row.maxTokens === 'number' && Number.isFinite(row.maxTokens)
        ? row.maxTokens
        : 8192,
    status: row.status ?? 'idle',
  }));
}

function providerKeyOf(prov: ProviderConfig): string {
  if (prov.configKey && prov.configKey.trim()) return prov.configKey.trim();
  return prov.id.startsWith('prov-') ? prov.id.slice(5) : prov.id;
}

export function RoutingView({
  routing,
  providers = [],
  availableModels,
  activeSessionModel,
  onSaveRouting,
  onOpenProviders,
}: RoutingViewProps) {
  const [draft, setDraft] = useState<SubagentRouting[]>(() =>
    normalizeRows(routing)
  );
  const [saving, setSaving] = useState(false);
  const [savedNotice, setSavedNotice] = useState(false);
  const [showAdvanced, setShowAdvanced] = useState(false);

  useEffect(() => {
    setDraft(normalizeRows(routing));
  }, [routing]);

  const hasChanges = useMemo(
    () => JSON.stringify(draft) !== JSON.stringify(normalizeRows(routing)),
    [draft, routing]
  );

  const enabledProviders = useMemo(
    () => providers.filter((p) => p.enabled),
    [providers]
  );

  const providerGroups = useMemo(() => {
    const multi = enabledProviders.length > 1;
    return enabledProviders.map((prov) => {
      const key = providerKeyOf(prov);
      const models = Array.from(
        new Set([prov.defaultModel, ...prov.models].filter(Boolean))
      ).map((m) => ({
        value: multi ? `${key}:${m}` : m,
        bare: m,
        label: m,
        isDefault: m === prov.defaultModel,
      }));
      return {
        id: prov.id,
        name: prov.name,
        key,
        models,
      };
    });
  }, [enabledProviders]);

  const knownModelValues = useMemo(() => {
    const set = new Set<string>();
    for (const g of providerGroups) {
      for (const m of g.models) {
        set.add(m.value);
        set.add(m.bare);
        set.add(`${g.key}:${m.bare}`);
      }
    }
    if (providerGroups.length === 0) {
      for (const m of availableModels) {
        if (m) set.add(m);
      }
    }
    return set;
  }, [providerGroups, availableModels]);

  const updateRow = (idx: number, patch: Partial<SubagentRouting>) => {
    setDraft((prev) =>
      prev.map((row, i) => (i === idx ? { ...row, ...patch } : row))
    );
    setSavedNotice(false);
  };

  // Plug-and-play preset: everything inherits the active session model with Auto thinking.
  const applyPlugAndPlay = () => {
    setDraft((prev) =>
      prev.map((row) => ({
        ...row,
        primaryModel: row.role === 'Meowstro' ? activeSessionModel ?? '' : '',
        fallbackModel: '',
        reasoningEffort: 'auto',
        temperature: null,
      }))
    );
    setSavedNotice(false);
  };

  // Recommended coding split: subagents inherit session model, with smart per-role thinking effort.
  const applySmartCodingSplit = () => {
    setDraft((prev) =>
      prev.map((row) => ({
        ...row,
        primaryModel: row.role === 'Meowstro' ? activeSessionModel ?? row.primaryModel : '',
        reasoningEffort: defaultEffortForRole(row.role),
      }))
    );
    setSavedNotice(false);
  };

  const handleSave = async () => {
    setSaving(true);
    try {
      await onSaveRouting(draft);
      setSavedNotice(true);
      setTimeout(() => setSavedNotice(false), 2500);
    } finally {
      setSaving(false);
    }
  };

  const renderModelOptions = (currentVal: string) => {
    const isUnknown = Boolean(currentVal) && !knownModelValues.has(currentVal);
    return (
      <>
        {isUnknown && (
          <option value={currentVal}>
            {currentVal} (not in active providers)
          </option>
        )}
        {providerGroups.length > 0
          ? providerGroups.map((group) => (
              <optgroup key={group.id} label={group.name}>
                {group.models.map((m) => (
                  <option key={`${group.id}:${m.bare}`} value={m.value}>
                    {m.label}
                    {m.isDefault ? ' (default)' : ''}
                  </option>
                ))}
              </optgroup>
            ))
          : availableModels.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
      </>
    );
  };

  return (
    <div className="flex-1 overflow-y-auto px-4 sm:px-8 py-8">
      <div className="max-w-5xl mx-auto space-y-6">
        <header className="flex flex-wrap items-end justify-between gap-4 pb-5 border-b border-hairline">
          <div className="space-y-1.5">
            <h1 className="font-editorial text-4xl text-ink">
              The Clowder Routing Matrix
            </h1>
            <p className="text-sm text-body max-w-2xl leading-relaxed">
              Plug-and-play model routing: leave subagents on{' '}
              <span className="font-mono text-ink">Follow active model</span> or assign specialized models and thinking budgets per role.
            </p>
          </div>

          <div className="flex items-center gap-2">
            {onOpenProviders && (
              <button
                type="button"
                onClick={onOpenProviders}
                className="h-10 px-3.5 inline-flex items-center gap-2 rounded-md border border-hairline bg-canvas hover:bg-surface-soft text-xs font-medium text-ink transition-colors cursor-pointer"
              >
                <Server className="w-3.5 h-3.5 text-primary" />
                <span>Providers &amp; Models</span>
              </button>
            )}
            <button
              type="button"
              disabled={saving || !hasChanges}
              onClick={handleSave}
              className="h-10 px-4 inline-flex items-center gap-2 rounded-md bg-primary text-on-primary text-sm font-medium hover:bg-primary-active transition-colors cursor-pointer disabled:opacity-50"
            >
              {savedNotice ? (
                <Check className="w-4 h-4" />
              ) : (
                <Save className="w-4 h-4" />
              )}
              <span>
                {saving ? 'Saving...' : savedNotice ? 'Saved' : 'Save Matrix'}
              </span>
            </button>
          </div>
        </header>

        {/* Simple Presets & Advanced Toggle */}
        <section className="rounded-xl border border-hairline bg-surface-soft p-4 space-y-3">
          <div className="flex items-start gap-2.5 text-xs text-body leading-relaxed">
            <Info className="w-4 h-4 text-primary shrink-0 mt-0.5" />
            <div>
              <strong className="text-ink">Zero-config default:</strong>{' '}
              The <strong>Session Model</strong> in the top bar drives Meowstro, and any subagent set to{' '}
              <span className="font-mono text-ink">Follow active model</span> automatically uses it. Modern models only need{' '}
              <strong>Thinking</strong> effort — leave temperature at model default unless tuning a local Ollama/vLLM server.
            </div>
          </div>

          <div className="flex flex-wrap items-center justify-between gap-2 pt-2 border-t border-hairline">
            <div className="flex flex-wrap items-center gap-2">
              <button
                type="button"
                onClick={applyPlugAndPlay}
                className="h-8 px-3 inline-flex items-center gap-1.5 rounded-md border border-hairline bg-canvas hover:bg-surface-strong text-xs font-medium text-ink transition-colors cursor-pointer"
              >
                <Zap className="w-3.5 h-3.5 text-primary" />
                <span>Plug &amp; Play (All Follow Active)</span>
              </button>
              <button
                type="button"
                onClick={applySmartCodingSplit}
                className="h-8 px-3 inline-flex items-center gap-1.5 rounded-md border border-hairline bg-canvas hover:bg-surface-strong text-xs font-medium text-ink transition-colors cursor-pointer"
              >
                <Sparkles className="w-3.5 h-3.5 text-primary" />
                <span>Smart Thinking Split</span>
              </button>
              <button
                type="button"
                onClick={() => setShowAdvanced((v) => !v)}
                className={`h-8 px-3 inline-flex items-center gap-1.5 rounded-md border text-xs font-medium transition-colors cursor-pointer ${
                  showAdvanced
                    ? 'border-primary bg-surface-card text-ink'
                    : 'border-hairline bg-canvas text-muted hover:text-ink'
                }`}
              >
                <SlidersHorizontal className="w-3.5 h-3.5" />
                <span>
                  {showAdvanced
                    ? 'Hide Advanced (Fallback & Temp)'
                    : 'Advanced (Fallback & Local Temp)'}
                </span>
              </button>
            </div>

            {hasChanges && (
              <div className="flex items-center gap-2">
                <span className="text-xs font-medium text-warning">
                  Unsaved changes
                </span>
                <button
                  type="button"
                  onClick={() => {
                    setDraft(normalizeRows(routing));
                    setSavedNotice(false);
                  }}
                  className="h-7 px-2 inline-flex items-center gap-1 rounded border border-hairline bg-canvas text-[11px] text-muted hover:text-ink cursor-pointer"
                >
                  <RotateCcw className="w-3 h-3" />
                  <span>Reset</span>
                </button>
              </div>
            )}
          </div>
        </section>

        <div className="rounded-xl border border-hairline bg-canvas overflow-hidden">
          <div className="overflow-x-auto">
            <table className="w-full text-left border-collapse">
              <thead>
                <tr className="border-b border-hairline bg-surface-soft text-[12px] font-medium text-muted">
                  <th className="py-3 px-4">Role</th>
                  <th className="py-3 px-4">Model</th>
                  <th className="py-3 px-4">Thinking Effort</th>
                  {showAdvanced && (
                    <>
                      <th className="py-3 px-4">Fallback Model</th>
                      <th className="py-3 px-4">Local Temperature</th>
                    </>
                  )}
                </tr>
              </thead>
              <tbody className="divide-y divide-hairline-soft text-sm">
                {draft.map((row, idx) => {
                  const isLead = row.role === 'Meowstro';
                  const primaryMissing =
                    Boolean(row.primaryModel) &&
                    !knownModelValues.has(row.primaryModel);
                  const tempEnabled =
                    typeof row.temperature === 'number' &&
                    Number.isFinite(row.temperature);
                  return (
                    <tr
                      key={row.role}
                      className="hover:bg-surface-soft/50 transition-colors"
                    >
                      <td className="py-3.5 px-4 align-top min-w-[200px]">
                        <div className="flex items-center gap-2">
                          <span className="font-medium text-ink">{row.role}</span>
                          <span
                            className={`px-1.5 py-0.5 rounded text-[10px] font-medium ${
                              isLead
                                ? 'bg-primary text-on-primary'
                                : 'bg-surface-soft border border-hairline text-muted'
                            }`}
                          >
                            {isLead ? 'Lead' : 'Subagent'}
                          </span>
                        </div>
                        <div className="text-xs text-muted mt-0.5">
                          {row.responsibility}
                        </div>
                      </td>

                      <td className="py-3.5 px-4 align-top">
                        <div className="space-y-1">
                          <select
                            aria-label={`${row.role} primary model`}
                            value={row.primaryModel}
                            onChange={(e) =>
                              updateRow(idx, { primaryModel: e.target.value })
                            }
                            className="h-9 w-full min-w-[210px] rounded-md border border-hairline bg-canvas px-2.5 font-mono text-xs text-ink focus:outline-none focus:border-primary"
                          >
                            <option value="">
                              {isLead
                                ? `Follow session model (${activeSessionModel || 'auto'})`
                                : 'Follow active model (inherit)'}
                            </option>
                            {renderModelOptions(row.primaryModel)}
                          </select>
                          {primaryMissing && (
                            <div className="inline-flex items-center gap-1 text-[11px] text-warning">
                              <AlertTriangle className="w-3 h-3 shrink-0" />
                              <span>Not in enabled providers</span>
                            </div>
                          )}
                        </div>
                      </td>

                      <td className="py-3.5 px-4 align-top">
                        <select
                          aria-label={`${row.role} thinking effort`}
                          value={row.reasoningEffort ?? 'auto'}
                          onChange={(e) =>
                            updateRow(idx, {
                              reasoningEffort: e.target.value as ReasoningEffort,
                            })
                          }
                          className="h-9 min-w-[175px] rounded-md border border-hairline bg-canvas px-2.5 text-xs text-ink focus:outline-none focus:border-primary"
                        >
                          {EFFORT_OPTIONS.map((opt) => (
                            <option key={opt.value} value={opt.value}>
                              {opt.label}
                            </option>
                          ))}
                        </select>
                      </td>

                      {showAdvanced && (
                        <>
                          <td className="py-3.5 px-4 align-top">
                            <select
                              aria-label={`${row.role} fallback model`}
                              value={row.fallbackModel}
                              onChange={(e) =>
                                updateRow(idx, { fallbackModel: e.target.value })
                              }
                              className="h-9 w-full min-w-[180px] rounded-md border border-hairline bg-canvas px-2.5 font-mono text-xs text-ink focus:outline-none focus:border-primary"
                            >
                              <option value="">None (no fallback)</option>
                              {renderModelOptions(row.fallbackModel)}
                            </select>
                          </td>

                          <td className="py-3.5 px-4 align-top">
                            <div className="flex items-center gap-2 min-w-[170px]">
                              <label className="inline-flex items-center gap-1.5 text-xs text-muted cursor-pointer whitespace-nowrap">
                                <input
                                  type="checkbox"
                                  checked={tempEnabled}
                                  onChange={(e) =>
                                    updateRow(idx, {
                                      temperature: e.target.checked ? 0.2 : null,
                                    })
                                  }
                                />
                                <span>{tempEnabled ? '' : 'Auto'}</span>
                              </label>
                              {tempEnabled && (
                                <>
                                  <input
                                    type="range"
                                    min={0}
                                    max={1.5}
                                    step={0.05}
                                    value={row.temperature ?? 0.2}
                                    onChange={(e) =>
                                      updateRow(idx, {
                                        temperature: parseFloat(e.target.value),
                                      })
                                    }
                                    className="w-20 accent-primary cursor-pointer"
                                  />
                                  <span className="font-mono text-xs text-ink tabular-nums w-8">
                                    {(row.temperature ?? 0.2).toFixed(2)}
                                  </span>
                                </>
                              )}
                            </div>
                          </td>
                        </>
                      )}
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </div>
  );
}
