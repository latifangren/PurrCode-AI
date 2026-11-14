import React, { useState } from 'react';
import { Activity, Plus, RefreshCw, Trash2 } from 'lucide-react';
import type { ProviderConfig } from '../types/cockpit';
import { fetchProviderModels, type FetchedModel } from '../lib/apiClient';

interface ProvidersViewProps {
  providers: ProviderConfig[];
  onAddProvider: (payload: {
    name: string;
    protocol: ProviderConfig['protocol'];
    baseUrl: string;
    defaultModel: string;
    contextWindow: number;
  }) => Promise<void>;
  onToggleProvider: (id: string, enabled: boolean) => Promise<void>;
  onDeleteProvider: (id: string) => Promise<void>;
  onTestLatency: (id: string) => Promise<void>;
}

export function ProvidersView({
  providers,
  onAddProvider,
  onToggleProvider,
  onDeleteProvider,
  onTestLatency,
}: ProvidersViewProps) {
  const [name, setName] = useState('');
  const [protocol, setProtocol] = useState<ProviderConfig['protocol']>('ollama');
  const [baseUrl, setBaseUrl] = useState('');
  const [defaultModel, setDefaultModel] = useState('');
  const [contextWindow, setContextWindow] = useState('64000');
  const [testingId, setTestingId] = useState<string | null>(null);
  const [fetchingId, setFetchingId] = useState<string | null>(null);
  const [fetchedModels, setFetchedModels] = useState<Record<string, FetchedModel[]>>({});
  const [selectedModelByProvider, setSelectedModelByProvider] = useState<Record<string, string>>({});
  const [fetchError, setFetchError] = useState<Record<string, string>>({});
  const [formError, setFormError] = useState<string | null>(null);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim() || !baseUrl.trim() || !defaultModel.trim()) {
      setFormError('Provider name, endpoint URL, and default model are required.');
      return;
    }
    setFormError(null);
    await onAddProvider({
      name: name.trim(),
      protocol,
      baseUrl: baseUrl.trim(),
      defaultModel: defaultModel.trim(),
      contextWindow: Number(contextWindow) || 64000,
    });
    setName('');
    setBaseUrl('');
    setDefaultModel('');
  };

  const handlePing = async (id: string) => {
    setTestingId(id);
    await onTestLatency(id);
    setTestingId(null);
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

  return (
    <div className="flex-1 overflow-y-auto px-4 sm:px-8 py-8">
      <div className="max-w-5xl mx-auto space-y-8">
        <header className="pb-5 border-b border-hairline space-y-1.5">
          <h1 className="font-editorial text-4xl text-ink">
            Providers & Model Endpoints
          </h1>
          <p className="text-sm text-body max-w-2xl leading-relaxed">
            Configure model gateway endpoints, context window budgets, and run live round-trip latency diagnostics. Credentials are managed securely through environment secrets.
          </p>
        </header>

        {/* Provider Cards Grid */}
        <div className="grid grid-cols-1 md:grid-cols-2 gap-5">
          {providers.map((prov) => (
            <div
              key={prov.id}
              className="rounded-xl border border-hairline bg-surface-card p-6 flex flex-col justify-between gap-4"
            >
              <div className="space-y-2.5">
                <div className="flex items-start justify-between gap-3">
                  <div>
                    <h2 className="font-sans text-base font-medium text-ink">
                      {prov.name}
                    </h2>
                    <div className="text-xs text-muted mt-0.5">
                      Protocol: {prov.protocol} · {(prov.contextWindow / 1000).toFixed(0)}k context
                    </div>
                  </div>

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

                <div className="rounded-md bg-code-bg text-code-text px-3.5 py-2.5 space-y-1 font-mono text-xs">
                  <div className="text-code-muted truncate">{prov.baseUrl}</div>
                  <div className="text-code-text">Default: {prov.defaultModel}</div>
                </div>
              </div>

              <div className="flex items-center justify-between gap-3 pt-2 border-t border-hairline">
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
                    className="h-9 px-3 inline-flex items-center gap-1.5 rounded-md border border-hairline bg-canvas hover:bg-surface-strong text-xs font-medium text-ink transition-colors whitespace-nowrap cursor-pointer disabled:opacity-50"
                  >
                    <RefreshCw className={`w-3.5 h-3.5 text-primary ${fetchingId === prov.id ? 'animate-spin' : ''}`} />
                    <span>{fetchingId === prov.id ? 'Fetching...' : 'Fetch Models'}</span>
                  </button>

                  <button
                    type="button"
                    disabled={testingId === prov.id}
                    onClick={() => handlePing(prov.id)}
                    className="h-9 px-3 inline-flex items-center gap-1.5 rounded-md border border-hairline bg-canvas hover:bg-surface-strong text-xs font-medium text-ink transition-colors whitespace-nowrap cursor-pointer"
                  >
                    <Activity className="w-3.5 h-3.5 text-primary" />
                    <span>{testingId === prov.id ? 'Pinging...' : 'Test Latency'}</span>
                  </button>

                  {providers.length > 1 && (
                    <button
                      type="button"
                      onClick={() => onDeleteProvider(prov.id)}
                      aria-label={`Remove provider ${prov.name}`}
                      className="h-9 w-9 inline-flex items-center justify-center rounded-md border border-hairline bg-canvas text-muted hover:text-error transition-colors cursor-pointer"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </button>
                  )}
                </div>
              </div>

              {fetchedModels[prov.id] && fetchedModels[prov.id].length > 0 && (
                <div className="pt-3 border-t border-hairline space-y-2">
                  <div className="flex items-center justify-between text-xs text-muted">
                    <span>Discovered Models ({fetchedModels[prov.id].length}):</span>
                    {selectedModelByProvider[prov.id] && (
                      <span className="font-mono text-[11px] text-body">
                        {(() => {
                          const found = fetchedModels[prov.id].find(
                            (m) => m.id === selectedModelByProvider[prov.id]
                          );
                          return found
                            ? `${(found.context_window / 1000).toFixed(0)}k ctx`
                            : '';
                        })()}
                      </span>
                    )}
                  </div>
                  <select
                    value={selectedModelByProvider[prov.id] || prov.defaultModel}
                    onChange={(e) =>
                      setSelectedModelByProvider((prev) => ({
                        ...prev,
                        [prov.id]: e.target.value,
                      }))
                    }
                    className="w-full h-8 px-2.5 rounded-md border border-hairline bg-canvas text-xs font-mono text-ink focus:outline-none focus:border-primary"
                  >
                    {fetchedModels[prov.id].map((m) => (
                      <option key={m.id} value={m.id}>
                        {m.id} {m.name && m.name !== m.id ? `(${m.name})` : ''}
                      </option>
                    ))}
                  </select>
                </div>
              )}

              {fetchError[prov.id] && (
                <div className="pt-2 text-xs text-error font-medium">
                  {fetchError[prov.id]}
                </div>
              )}
            </div>
          ))}
        </div>

        {/* Register Custom Endpoint Form */}
        <form
          onSubmit={handleSubmit}
          className="rounded-xl border border-hairline bg-canvas p-6 space-y-4"
        >
          <h2 className="font-editorial text-2xl text-ink">
            Register Model Endpoint
          </h2>

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
                onChange={(e) => setProtocol(e.target.value as ProviderConfig['protocol'])}
                className="h-10 w-full rounded-md border border-hairline bg-canvas px-3 text-sm text-ink focus:outline-none focus:border-primary"
              >
                <option value="ollama">Ollama</option>
                <option value="openai">OpenAI-Compatible</option>
                <option value="deepseek">DeepSeek</option>
                <option value="anthropic">Anthropic</option>
                <option value="gemini">Google Gemini</option>
                <option value="custom">Custom HTTP</option>
              </select>
            </label>

            <label className="space-y-1.5 text-xs font-medium text-body">
              <span>Default Model ID</span>
              <input
                type="text"
                value={defaultModel}
                onChange={(e) => setDefaultModel(e.target.value)}
                placeholder="e.g. qwen-2.5-coder-32b"
                className="h-10 w-full rounded-md border border-hairline bg-canvas px-3 font-mono text-xs text-ink focus:outline-none focus:border-primary"
              />
            </label>

            <label className="space-y-1.5 text-xs font-medium text-body sm:col-span-2">
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
              <span>Context Window (Tokens)</span>
              <input
                type="number"
                value={contextWindow}
                onChange={(e) => setContextWindow(e.target.value)}
                className="h-10 w-full rounded-md border border-hairline bg-canvas px-3 font-mono text-xs text-ink tabular-nums focus:outline-none focus:border-primary"
              />
            </label>
          </div>

          <div className="pt-2">
            <button
              type="submit"
              className="h-10 px-5 inline-flex items-center gap-2 rounded-md bg-primary hover:bg-primary-active text-on-primary text-sm font-medium transition-colors cursor-pointer"
            >
              <Plus className="w-4 h-4" />
              <span>Add Endpoint</span>
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
