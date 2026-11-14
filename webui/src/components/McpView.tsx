import React, { useState } from 'react';
import { Activity, Plus, Trash2 } from 'lucide-react';
import type { McpServer } from '../types/cockpit';

interface McpViewProps {
  servers: McpServer[];
  onAddServer: (payload: {
    name: string;
    transport: 'stdio' | 'sse';
    endpoint: string;
  }) => Promise<void>;
  onPingServer: (id: string) => Promise<void>;
  onDeleteServer: (id: string) => Promise<void>;
}

export function McpView({
  servers,
  onAddServer,
  onPingServer,
  onDeleteServer,
}: McpViewProps) {
  const [name, setName] = useState('');
  const [transport, setTransport] = useState<'stdio' | 'sse'>('stdio');
  const [endpoint, setEndpoint] = useState('');
  const [selectedServerId, setSelectedServerId] = useState<string>(servers[0]?.id ?? '');
  const [pingingId, setPingingId] = useState<string | null>(null);

  const activeServer = servers.find((s) => s.id === selectedServerId) ?? servers[0];

  const handlePing = async (id: string) => {
    setPingingId(id);
    await onPingServer(id);
    setPingingId(null);
  };

  const handleAdd = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim() || !endpoint.trim()) return;
    await onAddServer({ name: name.trim(), transport, endpoint: endpoint.trim() });
    setName('');
    setEndpoint('');
  };

  return (
    <div className="flex-1 overflow-y-auto px-4 sm:px-8 py-8">
      <div className="max-w-5xl mx-auto space-y-8">
        <header className="pb-5 border-b border-hairline space-y-1.5">
          <h1 className="font-editorial text-4xl text-ink">
            MCP Server Hub
          </h1>
          <p className="text-sm text-body max-w-2xl leading-relaxed">
            Connect Model Context Protocol servers over Stdio or SSE and inspect exported JSON tool schemas in real time.
          </p>
        </header>

        <div className="grid grid-cols-1 lg:grid-cols-5 gap-6">
          {/* Server List */}
          <div className="lg:col-span-2 space-y-4">
            <div className="space-y-3">
              {servers.map((srv) => {
                const isSelected = activeServer?.id === srv.id;
                return (
                  <div
                    key={srv.id}
                    className={`rounded-xl border p-4 space-y-3 transition-colors ${
                      isSelected
                        ? 'border-primary bg-surface-card'
                        : 'border-hairline bg-canvas'
                    }`}
                  >
                    <button
                      type="button"
                      onClick={() => setSelectedServerId(srv.id)}
                      className="w-full text-left space-y-1 cursor-pointer"
                    >
                      <div className="flex items-center justify-between gap-2">
                        <span className="font-medium text-sm text-ink">{srv.name}</span>
                        <span className="font-mono text-[11px] text-success">
                          {srv.status}
                        </span>
                      </div>
                      <div className="font-mono text-[11px] text-muted truncate">
                        {srv.transport.toUpperCase()} · {srv.endpoint}
                      </div>
                    </button>

                    <div className="flex items-center justify-between pt-2 border-t border-hairline-soft text-xs">
                      <span className="font-mono text-muted tabular-nums">
                        {srv.tools.length} tools · {srv.latencyMs ?? '—'}ms
                      </span>
                      <div className="flex items-center gap-1.5">
                        <button
                          type="button"
                          disabled={pingingId === srv.id}
                          onClick={() => handlePing(srv.id)}
                          aria-label={`Ping ${srv.name}`}
                          className="h-7 px-2.5 inline-flex items-center gap-1 rounded border border-hairline bg-canvas hover:bg-surface-strong disabled:opacity-60 text-xs text-ink cursor-pointer"
                        >
                          <Activity
                            className={`w-3 h-3 text-primary ${
                              pingingId === srv.id ? 'animate-spin' : ''
                            }`}
                          />
                          <span>{pingingId === srv.id ? 'Pinging...' : 'Ping'}</span>
                        </button>
                        <button
                          type="button"
                          onClick={() => onDeleteServer(srv.id)}
                          aria-label={`Delete MCP server ${srv.name}`}
                          className="h-7 w-7 inline-flex items-center justify-center rounded border border-hairline bg-canvas text-muted hover:text-error cursor-pointer"
                        >
                          <Trash2 className="w-3 h-3" />
                        </button>
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>

            {/* Add MCP Server Form */}
            <form
              onSubmit={handleAdd}
              className="rounded-xl border border-hairline bg-canvas p-4 space-y-3"
            >
              <div className="text-xs font-medium text-ink">Register MCP Server</div>
              <input
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="Server name (e.g. postgres-readonly)"
                aria-label="MCP server name"
                className="h-9 w-full rounded-md border border-hairline bg-canvas px-3 text-xs text-ink focus:outline-none focus:border-primary"
              />
              <select
                value={transport}
                onChange={(e) => setTransport(e.target.value as 'stdio' | 'sse')}
                aria-label="MCP transport"
                className="h-9 w-full rounded-md border border-hairline bg-canvas px-3 text-xs text-ink focus:outline-none focus:border-primary"
              >
                <option value="stdio">Stdio Process</option>
                <option value="sse">SSE Endpoint</option>
              </select>
              <input
                type="text"
                value={endpoint}
                onChange={(e) => setEndpoint(e.target.value)}
                placeholder={transport === 'stdio' ? 'npx -y @mcp/server-git' : 'http://127.0.0.1:8765/sse'}
                aria-label="MCP command or URL"
                className="h-9 w-full rounded-md border border-hairline bg-canvas px-3 font-mono text-xs text-ink focus:outline-none focus:border-primary"
              />
              <button
                type="submit"
                className="h-9 w-full inline-flex items-center justify-center gap-1.5 rounded-md bg-primary hover:bg-primary-active text-on-primary text-xs font-medium transition-colors cursor-pointer"
              >
                <Plus className="w-3.5 h-3.5" />
                <span>Connect Server</span>
              </button>
            </form>
          </div>

          {/* Live Tool Schema Discovery Panel */}
          <div className="lg:col-span-3 rounded-xl border border-code-border bg-code-bg text-code-text overflow-hidden flex flex-col">
            <div className="px-5 py-3.5 bg-code-elevated flex items-center justify-between">
              <div className="font-mono text-xs text-code-text">
                {activeServer
                  ? `Exported Tool Schemas · ${activeServer.name}`
                  : 'Select an MCP server'}
              </div>
              {activeServer && (
                <span className="font-mono text-[11px] text-code-muted tabular-nums">
                  {activeServer.tools.length} schemas
                </span>
              )}
            </div>

            <div className="p-5 space-y-4 overflow-y-auto dark-scroll">
              {!activeServer ? (
                <p className="text-sm text-code-muted">
                  Register or select an MCP server to inspect its JSON schemas.
                </p>
              ) : (
                activeServer.tools.map((tool) => (
                  <div
                    key={tool.name}
                    className="rounded-lg bg-code-soft border border-code-border p-4 space-y-2.5"
                  >
                    <div className="font-mono text-sm text-primary font-medium">
                      {tool.name}
                    </div>
                    <p className="text-xs text-code-text/85 leading-relaxed">
                      {tool.description}
                    </p>
                    <pre className="rounded bg-code-bg p-3 font-mono text-[12px] text-code-muted overflow-x-auto">
                      {JSON.stringify(tool.inputSchema, null, 2)}
                    </pre>
                  </div>
                ))
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
