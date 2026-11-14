import React, { useMemo, useState } from 'react';
import {
  Activity,
  Check,
  Edit3,
  KeyRound,
  Layers,
  Plus,
  RefreshCw,
  Trash2,
  X,
} from 'lucide-react';
import type { ProviderConfig } from '../types/cockpit';
import { fetchProviderModels, type FetchedModel } from '../lib/apiClient';

interface ProvidersViewProps {
  providers: ProviderConfig[];
  activeSessionModel?: string;
  onUseModel?: (model: string) => void;
  onOpenRouting?: () => void;
  onAddProvider: (payload: {
    name: string;
    protocol: ProviderConfig['protocol'];
    baseUrl: string;
    defaultModel: string;
    contextWindow: number;
    apiKey?: string;
  }) => Promise<void>;
  onUpdateProvider: (
    id: string,
    patch: {
      enabled?: boolean;
      name?: string;
      protocol?: ProviderConfig['protocol'];
      baseUrl?: string;
      defaultModel?: string;
      models?: string[];
      contextWindow?: number;
      apiKey?: string;
      clearApiKey?: boolean;
    },
    toastLabel?: string
  ) => Promise<void>;
  onToggleProvider: (id: string, enabled: boolean) => Promise<void>;
  onDeleteProvider: (id: string) => Promise<void>;
  onTestLatency: (id: string) => Promise<void>;
}

function providerKeyOf(prov: ProviderConfig): string {
  if (prov.configKey && prov.configKey.trim()) return prov.configKey.trim();
  return prov.id.startsWith('prov-') ? prov.id.slice(5) : prov.id;
}

function modelMatchesActive(
  activeModel: string | undefined,
  prov: ProviderConfig,
  modelId: string
): boolean {
  if (!activeModel) return false;
  const key = providerKeyOf(prov);
  return activeModel === modelId || activeModel === `${key}:${modelId}`;
}

export function ProvidersView({
  providers,
  activeSessionModel,
  onUseModel,
  onOpenRouting,
  onAddProvider,
  onUpdateProvider,
  onToggleProvider,
  onDeleteProvider,
  onTestLatency,
}: ProvidersViewProps) {
  const [name, setName] = useState('');
  const [protocol, setProtocol] = useState<ProviderConfig['protocol']>('openai');
  const [baseUrl, setBaseUrl] = useState('');
  const [defaultModel, setDefaultModel] = useState('');
  const [contextWindow, setContextWindow] = useState('131072');
  const [apiKeyInput, setApiKeyInput] = useState('');
  const [formError, setFormError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);

  const [testingId, setTestingId] = useState<string | null>(null);
  const [fetchingId, setFetchingId] = useState<string | null>(null);
  const [fetchedModels, setFetchedModels] = useState<Record<string, FetchedModel[]>>({});
  const [selectedModelByProvider, setSelectedModelByProvider] = useState<Record<string, string>>({});
  const [fetchFilter, setFetchFilter] = useState<Record<string, string>>({});
  const [fetchError, setFetchError] = useState<Record<string, string>>({});

  const [manualModelDraft, setManualModelDraft] = useState<Record<string, string>>({});
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editDraft, setEditDraft] = useState<{
    name: string;
    protocol: ProviderConfig['protocol'];
    baseUrl: string;
    defaultModel: string;
    contextWindow: string;
    apiKey: string;
    clearApiKey: boolean;
  } | null>(null);

  const enabledProviders = useMemo(
    () => providers.filter((p) => p.enabled),
    [providers]
  );

  const activeProvider = useMemo(() => {
    if (!activeSessionModel) return undefined;
    return enabledProviders.find((p) =>
      p.models.some((m) => modelMatchesActive(activeSessionModel, p, m))
    );
  }, [enabledProviders, activeSessionModel]);

  const qualifyModelForSession = (prov: ProviderConfig, modelId: string): string => {
    // Use provider:model when multiple enabled providers exist so routing is unambiguous.
    if (enabledProviders.length > 1) {
      return `${providerKeyOf(prov)}:${modelId}`;
    }
    return modelId;
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim() || !baseUrl.trim() || !defaultModel.trim()) {
      setFormError('Provider name, endpoint URL, and default model are required.');
      return;
    }
    setFormError(null);
    setAdding(true);
    try {
      await onAddProvider({
        name: name.trim(),
        protocol,
        baseUrl: baseUrl.trim(),
        defaultModel: defaultModel.trim(),
        contextWindow: Number(contextWindow) || 131072,
        apiKey: apiKeyInput.trim() || undefined,
      });
      setName('');
      setBaseUrl('');
      setDefaultModel('');
      setApiKeyInput('');
    } finally {
      setAdding(false);
    }
  };

  const handlePing = async (id: string) => {
    setTestingId(id);
    try {
      await onTestLatency(id);
    } finally {
      setTestingId(null);
    }
  };

  const handleFetchModels = async (provId: string) => {
    setFetchingId(provId);
    setFetchError((prev) => {
      const copy = { ...prev };
      delete copy[provId];
      return copy;
    });
    try {
      const models = await fetchProviderModels(provId);
      setFetchedModels((prev) => ({ ...prev, [provId]: models }));
      if (models.length > 0) {
        setSelectedModelByProvider((prev) => ({
          ...prev,
          [provId]: prev[provId] || models[0].id,
        }));
      }
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : 'Failed to fetch models';
      setFetchError((prev) => ({ ...prev, [provId]: msg }));
    } finally {
      setFetchingId(null);
    }
  };

  const handleSyncAllFetched = async (prov: ProviderConfig) => {
    const discovered = fetchedModels[prov.id] ?? [];
    if (discovered.length === 0) return;
    const merged = Array.from(
      new Set([...prov.models, ...discovered.map((m) => m.id)])
    );
    const maxCtx = discovered.reduce(
      (acc, m) => (m.context_window > acc ? m.context_window : acc),
      prov.contextWindow
    );
    await onUpdateProvider(
      prov.id,
      { models: merged, contextWindow: maxCtx },
      `Saved ${discovered.length} discovered models`
    );
  };

  const handleAddSingleModel = async (prov: ProviderConfig, modelId: string) => {
    const trimmed = modelId.trim();
    if (!trimmed) return;
    const nextModels = Array.from(new Set([...prov.models, trimmed]));
    await onUpdateProvider(
      prov.id,
      { models: nextModels },
      `Added ${trimmed}`
    );
  };

  const handleRemoveModel = async (prov: ProviderConfig, modelId: string) => {
    if (prov.models.length <= 1) return;
    const nextModels = prov.models.filter((m) => m !== modelId);
    const patch: { models: string[]; defaultModel?: string } = { models: nextModels };
    if (prov.defaultModel === modelId && nextModels.length > 0) {
      patch.defaultModel = nextModels[0];
    }
    await onUpdateProvider(prov.id, patch, `Removed ${modelId}`);
  };

  const startEditing = (prov: ProviderConfig) => {
    setEditingId(prov.id);
    setEditDraft({
      name: prov.name,
      protocol: prov.protocol,
      baseUrl: prov.baseUrl,
      defaultModel: prov.defaultModel,
      contextWindow: String(prov.contextWindow || 131072),
      apiKey: '',
      clearApiKey: false,
    });
  };

  const saveEdit = async (prov: ProviderConfig) => {
    if (!editDraft) return;
    const trimmedUrl = editDraft.baseUrl.trim();
    const trimmedName = editDraft.name.trim();
    const trimmedDefault = editDraft.defaultModel.trim();
    if (!trimmedUrl || !trimmedName || !trimmedDefault) return;
    const nextModels = Array.from(new Set([trimmedDefault, ...prov.models]));
    await onUpdateProvider(
      prov.id,
      {
        name: trimmedName,
        protocol: editDraft.protocol,
        baseUrl: trimmedUrl,
        defaultModel: trimmedDefault,
        models: nextModels,
        contextWindow: Number(editDraft.contextWindow) || prov.contextWindow,
        apiKey: editDraft.apiKey.trim() || undefined,
        clearApiKey: editDraft.clearApiKey,
      },
      `Saved ${trimmedName}`
    );
    setEditingId(null);
    setEditDraft(null);
  };

  return (
    <div className="flex-1 overflow-y-auto px-4 sm:px-8 py-8">
      <div className="max-w-5xl mx-auto space-y-7">
        <header className="flex flex-wrap items-end justify-between gap-4 pb-5 border-b border-hairline">
          <div className="space-y-1.5">
            <h1 className="font-editorial text-4xl text-ink">
              Providers &amp; Models
            </h1>
            <p className="text-sm text-body max-w-2xl leading-relaxed">
              Manage LLM gateway endpoints, sync available models, and switch which model powers the active chat session.
            </p>
          </div>
          {onOpenRouting && (
            <button
              type="button"
              onClick={onOpenRouting}
              className="h-9 px-3.5 inline-flex items-center gap-2 rounded-md border border-hairline bg-surface-card hover:bg-surface-strong text-xs font-medium text-ink transition-colors cursor-pointer"
            >
              <Layers className="w-3.5 h-3.5 text-primary" />
              <span>Clowder Routing Matrix</span>
            </button>
          )}
        </header>

        {/* Active Session Model Banner */}
        <section className="rounded-xl border border-hairline bg-surface-soft p-4 sm:p-5 flex flex-wrap items-center justify-between gap-4">
          <div className="space-y-1 min-w-0">
            <div className="flex items-center gap-2 text-xs font-medium text-muted">
              <span className="w-2 h-2 rounded-full bg-success shrink-0" />
              <span>Active Session Model</span>
              {activeProvider && (
                <span className="px-2 py-0.5 rounded bg-surface-card border border-hairline text-[11px] font-mono text-body">
                  {activeProvider.name} · {(activeProvider.contextWindow / 1000).toFixed(0)}k ctx
                </span>
              )}
            </div>
            <div className="font-mono text-sm sm:text-base font-medium text-ink truncate">
              {activeSessionModel || 'No model selected'}
            </div>
          </div>

          {onUseModel && enabledProviders.length > 0 && (
            <label className="flex items-center gap-2 text-xs text-muted">
              <span>Quick Switch:</span>
              <select
                aria-label="Quick switch active session model"
                value={activeSessionModel ?? ''}
                onChange={(e) => {
                  if (e.target.value) onUseModel(e.target.value);
                }}
                className="h-9 rounded-md border border-hairline bg-canvas px-3 font-mono text-xs text-ink focus:outline-none focus:border-primary max-w-[260px]"
              >
                {activeSessionModel &&
                  !enabledProviders.some((p) =>
                    p.models.some((m) => modelMatchesActive(activeSessionModel, p, m))
                  ) && (
                    <option value={activeSessionModel}>{activeSessionModel}</option>
                  )}
                {enabledProviders.map((prov) => (
                  <optgroup key={prov.id} label={prov.name}>
                    {Array.from(
                      new Set(
                        [prov.defaultModel, ...prov.models].filter(Boolean)
                      )
                    ).map((m) => {
                      const val = qualifyModelForSession(prov, m);
                      return (
                        <option key={`${prov.id}:${m}`} value={val}>
                          {m}
                          {m === prov.defaultModel ? ' (default)' : ''}
                        </option>
                      );
                    })}
                  </optgroup>
                ))}
              </select>
            </label>
          )}
        </section>

        {/* Provider Cards Grid */}
        {providers.length === 0 ? (
          <div className="rounded-xl border border-dashed border-hairline bg-surface-soft/50 p-8 text-center space-y-2">
            <div className="text-sm font-medium text-ink">
              No model providers configured yet
            </div>
            <p className="text-xs text-muted max-w-md mx-auto">
              Register an endpoint below (Ollama, OpenAI-compatible gateway, Magpie, Anthropic, Gemini, or DeepSeek) to start chatting.
            </p>
          </div>
        ) : (
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-5">
            {providers.map((prov) => {
              const allModels = Array.from(
                new Set([prov.defaultModel, ...prov.models].filter(Boolean))
              );
              const discovered = fetchedModels[prov.id] ?? [];
              const filterText = (fetchFilter[prov.id] ?? '').trim().toLowerCase();
              const visibleDiscovered = filterText
                ? discovered.filter(
                    (m) =>
                      m.id.toLowerCase().includes(filterText) ||
                      m.name.toLowerCase().includes(filterText)
                  )
                : discovered;
              const selectedFetched =
                selectedModelByProvider[prov.id] || discovered[0]?.id || '';
              const isEditing = editingId === prov.id && editDraft !== null;

              return (
                <div
                  key={prov.id}
                  className={`rounded-xl border bg-surface-card p-5 flex flex-col justify-between gap-4 transition-opacity ${
                    prov.enabled ? 'border-hairline' : 'border-hairline opacity-70'
                  }`}
                >
                  <div className="space-y-3">
                    {/* Header Row */}
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0 space-y-1">
                        <div className="flex flex-wrap items-center gap-2">
                          <h2 className="font-sans text-base font-medium text-ink truncate">
                            {prov.name}
                          </h2>
                          <span className="px-1.5 py-0.5 rounded text-[10px] font-mono border border-hairline bg-canvas text-muted">
                            {prov.source === 'config' ? 'config' : 'custom'}
                          </span>
                        </div>
                        <div className="flex flex-wrap items-center gap-2 text-xs text-muted">
                          <span className="font-mono text-[11px] text-body">
                            {prov.protocol}
                          </span>
                          <span>·</span>
                          <span>{(prov.contextWindow / 1000).toFixed(0)}k ctx</span>
                          {prov.hasApiKey && (
                            <>
                              <span>·</span>
                              <span className="inline-flex items-center gap-1 text-accent-teal">
                                <KeyRound className="w-3 h-3" />
                                Key set
                              </span>
                            </>
                          )}
                        </div>
                      </div>

                      <div className="flex items-center gap-1.5 shrink-0">
                        <button
                          type="button"
                          onClick={() =>
                            isEditing ? setEditingId(null) : startEditing(prov)
                          }
                          aria-label={`Edit provider ${prov.name}`}
                          className="h-8 px-2.5 inline-flex items-center gap-1 rounded-md border border-hairline bg-canvas hover:bg-surface-strong text-xs text-ink transition-colors cursor-pointer"
                        >
                          <Edit3 className="w-3 h-3" />
                          <span>{isEditing ? 'Cancel' : 'Edit'}</span>
                        </button>
                        <button
                          type="button"
                          onClick={() => onToggleProvider(prov.id, !prov.enabled)}
                          className={`h-8 px-3 rounded-md text-xs font-medium transition-colors cursor-pointer whitespace-nowrap ${
                            prov.enabled
                              ? 'bg-code-bg text-code-text'
                              : 'border border-hairline bg-canvas text-muted'
                          }`}
                        >
                          {prov.enabled ? 'Enabled' : 'Disabled'}
                        </button>
                      </div>
                    </div>

                    {/* Inline Edit Mode */}
                    {isEditing && editDraft ? (
                      <div className="rounded-lg border border-hairline bg-canvas p-3.5 space-y-3">
                        <div className="grid grid-cols-1 sm:grid-cols-2 gap-2.5">
                          <label className="space-y-1 text-[11px] font-medium text-body">
                            <span>Display Name</span>
                            <input
                              type="text"
                              value={editDraft.name}
                              onChange={(e) =>
                                setEditDraft({ ...editDraft, name: e.target.value })
                              }
                              className="h-8 w-full rounded border border-hairline bg-surface-soft px-2.5 text-xs text-ink"
                            />
                          </label>
                          <label className="space-y-1 text-[11px] font-medium text-body">
                            <span>Protocol</span>
                            <select
                              value={editDraft.protocol}
                              onChange={(e) =>
                                setEditDraft({
                                  ...editDraft,
                                  protocol: e.target.value as ProviderConfig['protocol'],
                                })
                              }
                              className="h-8 w-full rounded border border-hairline bg-surface-soft px-2 text-xs text-ink"
                            >
                              <option value="openai">OpenAI-Compatible</option>
                              <option value="ollama">Ollama</option>
                              <option value="anthropic">Anthropic</option>
                              <option value="gemini">Gemini</option>
                              <option value="deepseek">DeepSeek</option>
                              <option value="custom">Custom</option>
                            </select>
                          </label>
                          <label className="space-y-1 text-[11px] font-medium text-body sm:col-span-2">
                            <span>Base URL</span>
                            <input
                              type="url"
                              value={editDraft.baseUrl}
                              onChange={(e) =>
                                setEditDraft({ ...editDraft, baseUrl: e.target.value })
                              }
                              className="h-8 w-full rounded border border-hairline bg-surface-soft px-2.5 font-mono text-xs text-ink"
                            />
                          </label>
                          <label className="space-y-1 text-[11px] font-medium text-body">
                            <span>Default Model</span>
                            <input
                              type="text"
                              value={editDraft.defaultModel}
                              onChange={(e) =>
                                setEditDraft({
                                  ...editDraft,
                                  defaultModel: e.target.value,
                                })
                              }
                              className="h-8 w-full rounded border border-hairline bg-surface-soft px-2.5 font-mono text-xs text-ink"
                            />
                          </label>
                          <label className="space-y-1 text-[11px] font-medium text-body">
                            <span>Context Window</span>
                            <input
                              type="number"
                              min={1024}
                              value={editDraft.contextWindow}
                              onChange={(e) =>
                                setEditDraft({
                                  ...editDraft,
                                  contextWindow: e.target.value,
                                })
                              }
                              className="h-8 w-full rounded border border-hairline bg-surface-soft px-2.5 font-mono text-xs text-ink"
                            />
                          </label>
                          <label className="space-y-1 text-[11px] font-medium text-body sm:col-span-2">
                            <span>API Key or $ENV_VAR (leave blank to keep current)</span>
                            <input
                              type="password"
                              value={editDraft.apiKey}
                              onChange={(e) =>
                                setEditDraft({
                                  ...editDraft,
                                  apiKey: e.target.value,
                                  clearApiKey: false,
                                })
                              }
                              placeholder={
                                prov.hasApiKey ? '•••••••• (set)' : 'Optional: sk-... or $OPENAI_API_KEY'
                              }
                              className="h-8 w-full rounded border border-hairline bg-surface-soft px-2.5 font-mono text-xs text-ink"
                            />
                          </label>
                        </div>

                        <div className="flex items-center justify-between gap-2 pt-1">
                          {prov.hasApiKey ? (
                            <label className="inline-flex items-center gap-1.5 text-[11px] text-muted cursor-pointer">
                              <input
                                type="checkbox"
                                checked={editDraft.clearApiKey}
                                onChange={(e) =>
                                  setEditDraft({
                                    ...editDraft,
                                    clearApiKey: e.target.checked,
                                    apiKey: '',
                                  })
                                }
                              />
                              <span>Clear saved API key</span>
                            </label>
                          ) : (
                            <span />
                          )}
                          <button
                            type="button"
                            onClick={() => saveEdit(prov)}
                            className="h-8 px-3 rounded-md bg-primary text-on-primary text-xs font-medium hover:bg-primary-active transition-colors cursor-pointer"
                          >
                            Save Changes
                          </button>
                        </div>
                      </div>
                    ) : (
                      <div className="rounded-md bg-code-bg text-code-text px-3.5 py-2.5 space-y-1 font-mono text-xs">
                        <div className="text-code-muted truncate">{prov.baseUrl}</div>
                        <div className="text-code-text truncate">
                          Default: {prov.defaultModel || '—'}
                        </div>
                      </div>
                    )}

                    {/* Saved Models Roster */}
                    <div className="space-y-2">
                      <div className="flex items-center justify-between text-xs text-muted">
                        <span>Models ({allModels.length})</span>
                        <span className="font-mono text-[11px]">
                          prefix: {providerKeyOf(prov)}:
                        </span>
                      </div>

                      <div className="space-y-1.5 max-h-48 overflow-y-auto pr-0.5">
                        {allModels.map((modelId) => {
                          const isDefault = prov.defaultModel === modelId;
                          const isActive = modelMatchesActive(
                            activeSessionModel,
                            prov,
                            modelId
                          );
                          return (
                            <div
                              key={modelId}
                              className={`flex items-center justify-between gap-2 rounded-md border px-2.5 py-1.5 text-xs ${
                                isActive
                                  ? 'border-primary bg-surface-soft'
                                  : 'border-hairline bg-canvas'
                              }`}
                            >
                              <div className="flex items-center gap-1.5 min-w-0">
                                <span className="font-mono text-ink truncate">
                                  {modelId}
                                </span>
                                {isDefault && (
                                  <span className="px-1.5 py-0.5 rounded bg-surface-strong text-[10px] font-medium text-body shrink-0">
                                    default
                                  </span>
                                )}
                                {isActive && (
                                  <span className="px-1.5 py-0.5 rounded bg-primary text-[10px] font-medium text-on-primary shrink-0">
                                    active
                                  </span>
                                )}
                              </div>

                              <div className="flex items-center gap-1 shrink-0">
                                {onUseModel && prov.enabled && !isActive && (
                                  <button
                                    type="button"
                                    onClick={() =>
                                      onUseModel(qualifyModelForSession(prov, modelId))
                                    }
                                    className="h-6 px-2 rounded bg-surface-soft hover:bg-surface-strong border border-hairline text-[11px] font-medium text-ink transition-colors cursor-pointer"
                                  >
                                    Use
                                  </button>
                                )}
                                {!isDefault && (
                                  <button
                                    type="button"
                                    onClick={() =>
                                      onUpdateProvider(
                                        prov.id,
                                        { defaultModel: modelId },
                                        `Default set to ${modelId}`
                                      )
                                    }
                                    className="h-6 px-2 rounded border border-hairline bg-canvas hover:bg-surface-soft text-[11px] text-body transition-colors cursor-pointer"
                                  >
                                    Set Default
                                  </button>
                                )}
                                {allModels.length > 1 && (
                                  <button
                                    type="button"
                                    onClick={() => handleRemoveModel(prov, modelId)}
                                    aria-label={`Remove model ${modelId}`}
                                    className="h-6 w-6 inline-flex items-center justify-center rounded text-muted hover:text-error transition-colors cursor-pointer"
                                  >
                                    <X className="w-3 h-3" />
                                  </button>
                                )}
                              </div>
                            </div>
                          );
                        })}
                      </div>

                      {/* Quick manual model add */}
                      <div className="flex items-center gap-1.5 pt-0.5">
                        <input
                          type="text"
                          value={manualModelDraft[prov.id] ?? ''}
                          onChange={(e) =>
                            setManualModelDraft((prev) => ({
                              ...prev,
                              [prov.id]: e.target.value,
                            }))
                          }
                          onKeyDown={(e) => {
                            if (e.key === 'Enter') {
                              e.preventDefault();
                              const val = manualModelDraft[prov.id] ?? '';
                              if (val.trim()) {
                                void handleAddSingleModel(prov, val);
                                setManualModelDraft((prev) => ({
                                  ...prev,
                                  [prov.id]: '',
                                }));
                              }
                            }
                          }}
                          placeholder="Add model ID manually (e.g. gpt-4o)..."
                          className="h-8 flex-1 rounded-md border border-hairline bg-canvas px-2.5 font-mono text-xs text-ink focus:outline-none focus:border-primary"
                        />
                        <button
                          type="button"
                          onClick={() => {
                            const val = manualModelDraft[prov.id] ?? '';
                            if (val.trim()) {
                              void handleAddSingleModel(prov, val);
                              setManualModelDraft((prev) => ({
                                ...prev,
                                [prov.id]: '',
                              }));
                            }
                          }}
                          className="h-8 px-2.5 rounded-md border border-hairline bg-canvas hover:bg-surface-strong text-xs font-medium text-ink transition-colors cursor-pointer"
                        >
                          Add
                        </button>
                      </div>
                    </div>
                  </div>

                  {/* Footer Actions */}
                  <div className="space-y-3 pt-2 border-t border-hairline">
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <div className="font-mono text-xs text-body tabular-nums">
                        {prov.lastLatencyMs !== null
                          ? `Ping: ${prov.lastLatencyMs}ms (${prov.lastTestedAt})`
                          : 'Not tested yet'}
                      </div>

                      <div className="flex items-center gap-2">
                        <button
                          type="button"
                          disabled={fetchingId === prov.id}
                          onClick={() => handleFetchModels(prov.id)}
                          aria-label={`Fetch models for ${prov.name}`}
                          className="h-8 px-2.5 inline-flex items-center gap-1.5 rounded-md border border-hairline bg-canvas hover:bg-surface-strong text-xs font-medium text-ink transition-colors whitespace-nowrap cursor-pointer disabled:opacity-50"
                        >
                          <RefreshCw
                            className={`w-3.5 h-3.5 text-primary ${
                              fetchingId === prov.id ? 'animate-spin' : ''
                            }`}
                          />
                          <span>
                            {fetchingId === prov.id ? 'Fetching...' : 'Fetch Models'}
                          </span>
                        </button>

                        <button
                          type="button"
                          disabled={testingId === prov.id}
                          onClick={() => handlePing(prov.id)}
                          className="h-8 px-2.5 inline-flex items-center gap-1.5 rounded-md border border-hairline bg-canvas hover:bg-surface-strong text-xs font-medium text-ink transition-colors whitespace-nowrap cursor-pointer"
                        >
                          <Activity className="w-3.5 h-3.5 text-primary" />
                          <span>
                            {testingId === prov.id ? 'Pinging...' : 'Ping'}
                          </span>
                        </button>

                        {prov.source !== 'config' && providers.length > 1 && (
                          <button
                            type="button"
                            onClick={() => onDeleteProvider(prov.id)}
                            aria-label={`Remove provider ${prov.name}`}
                            className="h-8 w-8 inline-flex items-center justify-center rounded-md border border-hairline bg-canvas text-muted hover:text-error transition-colors cursor-pointer"
                          >
                            <Trash2 className="w-3.5 h-3.5" />
                          </button>
                        )}
                      </div>
                    </div>

                    {discovered.length > 0 && (
                      <div className="rounded-lg border border-hairline bg-canvas p-3 space-y-2">
                        <div className="flex flex-wrap items-center justify-between gap-2 text-xs">
                          <span className="font-medium text-ink">
                            Discovered from /models ({discovered.length})
                          </span>
                          <button
                            type="button"
                            onClick={() => handleSyncAllFetched(prov)}
                            className="h-7 px-2.5 inline-flex items-center gap-1 rounded bg-primary text-on-primary text-[11px] font-medium hover:bg-primary-active transition-colors cursor-pointer"
                          >
                            <Check className="w-3 h-3" />
                            <span>Save All to Provider</span>
                          </button>
                        </div>

                        {discovered.length > 8 && (
                          <input
                            type="text"
                            value={fetchFilter[prov.id] ?? ''}
                            onChange={(e) =>
                              setFetchFilter((prev) => ({
                                ...prev,
                                [prov.id]: e.target.value,
                              }))
                            }
                            placeholder="Filter discovered models..."
                            className="h-7 w-full rounded border border-hairline bg-surface-soft px-2.5 text-xs text-ink"
                          />
                        )}

                        <div className="flex items-center gap-2">
                          <select
                            value={selectedFetched}
                            onChange={(e) =>
                              setSelectedModelByProvider((prev) => ({
                                ...prev,
                                [prov.id]: e.target.value,
                              }))
                            }
                            className="flex-1 h-8 px-2.5 rounded-md border border-hairline bg-surface-soft text-xs font-mono text-ink focus:outline-none focus:border-primary"
                          >
                            {visibleDiscovered.map((m) => (
                              <option key={m.id} value={m.id}>
                                {m.id}{' '}
                                {m.name && m.name !== m.id ? `(${m.name})` : ''}
                              </option>
                            ))}
                          </select>
                          <button
                            type="button"
                            disabled={!selectedFetched}
                            onClick={() =>
                              selectedFetched &&
                              handleAddSingleModel(prov, selectedFetched)
                            }
                            className="h-8 px-2.5 rounded-md border border-hairline bg-surface-soft hover:bg-surface-strong text-xs font-medium text-ink transition-colors cursor-pointer"
                          >
                            Add
                          </button>
                          {onUseModel && prov.enabled && selectedFetched && (
                            <button
                              type="button"
                              onClick={async () => {
                                await handleAddSingleModel(prov, selectedFetched);
                                onUseModel(
                                  qualifyModelForSession(prov, selectedFetched)
                                );
                              }}
                              className="h-8 px-2.5 rounded-md bg-code-bg text-code-text text-xs font-medium hover:opacity-90 transition-opacity cursor-pointer"
                            >
                              Use Now
                            </button>
                          )}
                        </div>
                      </div>
                    )}

                    {fetchError[prov.id] && (
                      <div className="text-xs text-error font-medium">
                        {fetchError[prov.id]}
                      </div>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        )}

        {/* Register Custom Endpoint Form */}
        <form
          onSubmit={handleSubmit}
          className="rounded-xl border border-hairline bg-canvas p-6 space-y-4"
        >
          <div className="space-y-1">
            <h2 className="font-editorial text-2xl text-ink">
              Register Model Endpoint
            </h2>
            <p className="text-xs text-muted">
              Add an Ollama, vLLM, OpenAI-compatible, Anthropic, Gemini, or DeepSeek gateway. API keys may be literal or an env var like <code className="font-mono">$OPENAI_API_KEY</code>.
            </p>
          </div>

          {formError && (
            <div role="alert" className="text-xs text-error font-medium">
              {formError}
            </div>
          )}

          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
            <label className="space-y-1.5 text-xs font-medium text-body">
              <span>Provider Name</span>
              <input
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="e.g. Local vLLM Cluster"
                className="h-10 w-full rounded-md border border-hairline bg-canvas px-3 text-sm text-ink focus:outline-none focus:border-primary"
              />
            </label>

            <label className="space-y-1.5 text-xs font-medium text-body">
              <span>Protocol</span>
              <select
                value={protocol}
                onChange={(e) =>
                  setProtocol(e.target.value as ProviderConfig['protocol'])
                }
                className="h-10 w-full rounded-md border border-hairline bg-canvas px-3 text-sm text-ink focus:outline-none focus:border-primary"
              >
                <option value="openai">OpenAI-Compatible</option>
                <option value="ollama">Ollama</option>
                <option value="deepseek">DeepSeek</option>
                <option value="anthropic">Anthropic</option>
                <option value="gemini">Gemini</option>
                <option value="custom">Custom</option>
              </select>
            </label>

            <label className="space-y-1.5 text-xs font-medium text-body">
              <span>Endpoint Base URL</span>
              <input
                type="url"
                value={baseUrl}
                onChange={(e) => setBaseUrl(e.target.value)}
                placeholder="http://127.0.0.1:11434/v1"
                className="h-10 w-full rounded-md border border-hairline bg-canvas px-3 font-mono text-xs text-ink focus:outline-none focus:border-primary"
              />
            </label>

            <label className="space-y-1.5 text-xs font-medium text-body">
              <span>Default Model ID</span>
              <input
                type="text"
                value={defaultModel}
                onChange={(e) => setDefaultModel(e.target.value)}
                placeholder="e.g. qwen2.5-coder:32b"
                className="h-10 w-full rounded-md border border-hairline bg-canvas px-3 font-mono text-xs text-ink focus:outline-none focus:border-primary"
              />
            </label>

            <label className="space-y-1.5 text-xs font-medium text-body">
              <span>Context Window (tokens)</span>
              <input
                type="number"
                min={4096}
                step={1024}
                value={contextWindow}
                onChange={(e) => setContextWindow(e.target.value)}
                className="h-10 w-full rounded-md border border-hairline bg-canvas px-3 font-mono text-xs text-ink focus:outline-none focus:border-primary"
              />
            </label>

            <label className="space-y-1.5 text-xs font-medium text-body">
              <span>API Key or $ENV_VAR (optional)</span>
              <input
                type="password"
                value={apiKeyInput}
                onChange={(e) => setApiKeyInput(e.target.value)}
                placeholder="sk-... or $OPENAI_API_KEY"
                className="h-10 w-full rounded-md border border-hairline bg-canvas px-3 font-mono text-xs text-ink focus:outline-none focus:border-primary"
              />
            </label>
          </div>

          <div className="flex justify-end">
            <button
              type="submit"
              disabled={adding}
              className="h-10 px-4 inline-flex items-center gap-2 rounded-md bg-primary text-on-primary text-sm font-medium hover:bg-primary-active transition-colors cursor-pointer disabled:opacity-50"
            >
              <Plus className="w-4 h-4" />
              <span>{adding ? 'Registering...' : 'Register Provider'}</span>
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
