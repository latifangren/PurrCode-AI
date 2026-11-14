import React, { useState } from 'react';
import { Check, Save } from 'lucide-react';
import type { SubagentRouting } from '../types/cockpit';

interface RoutingViewProps {
  routing: SubagentRouting[];
  availableModels: string[];
  onSaveRouting: (next: SubagentRouting[]) => Promise<void>;
}

export function RoutingView({ routing, availableModels, onSaveRouting }: RoutingViewProps) {
  const [draft, setDraft] = useState<SubagentRouting[]>(() => structuredClone(routing));
  const [saving, setSaving] = useState(false);
  const [savedNotice, setSavedNotice] = useState(false);

  const updateRow = (idx: number, patch: Partial<SubagentRouting>) => {
    setDraft((prev) => prev.map((row, i) => (i === idx ? { ...row, ...patch } : row)));
    setSavedNotice(false);
  };

  const handleSave = async () => {
    setSaving(true);
    await onSaveRouting(draft);
    setSaving(false);
    setSavedNotice(true);
    setTimeout(() => setSavedNotice(false), 2500);
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
              Assign primary models, fallback chains, temperature, and output token budgets per specialized subagent.
            </p>
          </div>

          <button
            type="button"
            disabled={saving}
            onClick={handleSave}
            className="h-10 px-5 inline-flex items-center gap-2 rounded-md bg-primary hover:bg-primary-active disabled:bg-primary-disabled text-on-primary text-sm font-medium transition-colors whitespace-nowrap shrink-0 cursor-pointer"
          >
            {savedNotice ? <Check className="w-4 h-4" /> : <Save className="w-4 h-4" />}
            <span>{saving ? 'Saving...' : savedNotice ? 'Saved' : 'Save Matrix'}</span>
          </button>
        </header>

        <div className="rounded-xl border border-hairline bg-canvas overflow-hidden">
          <div className="overflow-x-auto">
            <table className="w-full text-left border-collapse">
              <thead>
                <tr className="border-b border-hairline bg-surface-soft text-[12px] font-medium text-muted">
                  <th className="py-3 px-4">Subagent</th>
                  <th className="py-3 px-4">Primary Model</th>
                  <th className="py-3 px-4">Fallback Model</th>
                  <th className="py-3 px-4">Temperature</th>
                  <th className="py-3 px-4 text-right">Max Tokens</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-hairline-soft text-sm">
                {draft.map((row, idx) => (
                  <tr key={row.role} className="hover:bg-surface-soft/50 transition-colors">
                    <td className="py-3.5 px-4 align-top">
                      <div className="font-medium text-ink">{row.role}</div>
                      <div className="text-xs text-muted mt-0.5">{row.responsibility}</div>
                    </td>

                    <td className="py-3.5 px-4 align-top">
                      <select
                        aria-label={`${row.role} primary model`}
                        value={row.primaryModel}
                        onChange={(e) => updateRow(idx, { primaryModel: e.target.value })}
                        className="h-9 w-full min-w-[170px] rounded-md border border-hairline bg-canvas px-2.5 font-mono text-xs text-ink focus:outline-none focus:border-primary"
                      >
                        {Array.from(new Set([...availableModels, row.primaryModel])).map((m) => (
                          <option key={m} value={m}>
                            {m}
                          </option>
                        ))}
                      </select>
                    </td>

                    <td className="py-3.5 px-4 align-top">
                      <select
                        aria-label={`${row.role} fallback model`}
                        value={row.fallbackModel}
                        onChange={(e) => updateRow(idx, { fallbackModel: e.target.value })}
                        className="h-9 w-full min-w-[170px] rounded-md border border-hairline bg-canvas px-2.5 font-mono text-xs text-ink focus:outline-none focus:border-primary"
                      >
                        {Array.from(
                          new Set([
                            ...availableModels,
                            row.fallbackModel,
                            'deepseek-v3',
                            'deepseek-r1',
                            'claude-sonnet-4',
                            'qwen-2.5-coder-32b',
                            'ollama/llama3.3',
                          ])
                        ).map((m) => (
                          <option key={m} value={m}>
                            {m}
                          </option>
                        ))}
                      </select>
                    </td>

                    <td className="py-3.5 px-4 align-top">
                      <div className="flex items-center gap-2.5 min-w-[140px]">
                        <input
                          type="range"
                          min="0"
                          max="1"
                          step="0.05"
                          aria-label={`${row.role} temperature`}
                          value={row.temperature}
                          onChange={(e) => updateRow(idx, { temperature: Number(e.target.value) })}
                          className="w-24 accent-primary cursor-pointer"
                        />
                        <span className="font-mono text-xs text-ink tabular-nums w-8">
                          {row.temperature.toFixed(2)}
                        </span>
                      </div>
                    </td>

                    <td className="py-3.5 px-4 align-top text-right">
                      <input
                        type="number"
                        min={1024}
                        max={128000}
                        step={1024}
                        aria-label={`${row.role} max tokens`}
                        value={row.maxTokens}
                        onChange={(e) => updateRow(idx, { maxTokens: Number(e.target.value) || 4096 })}
                        className="h-9 w-28 rounded-md border border-hairline bg-canvas px-2.5 text-right font-mono text-xs text-ink tabular-nums focus:outline-none focus:border-primary"
                      />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </div>
  );
}
