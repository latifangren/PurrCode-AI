import React, { useState } from 'react';
import { Plus, Search, ShieldCheck, Trash2 } from 'lucide-react';
import { filterAuditEntries } from '../lib/cockpitLogic';
import type { AuditOutcome, SandboxState } from '../types/cockpit';

interface SandboxViewProps {
  sandbox: SandboxState;
  onAddPath: (path: string, mode: 'ro' | 'rw') => Promise<string | null>;
  onRemovePath: (id: string) => Promise<void>;
  onToggleOutbound: (enabled: boolean) => Promise<void>;
}

export function SandboxView({
  sandbox,
  onAddPath,
  onRemovePath,
  onToggleOutbound,
}: SandboxViewProps) {
  const [newPath, setNewPath] = useState('');
  const [newMode, setNewMode] = useState<'ro' | 'rw'>('ro');
  const [pathError, setPathError] = useState<string | null>(null);
  const [auditFilter, setAuditFilter] = useState<'All' | AuditOutcome>('All');
  const [auditQuery, setAuditQuery] = useState('');

  const handleAdd = async (e: React.FormEvent) => {
    e.preventDefault();
    const err = await onAddPath(newPath, newMode);
    if (err) {
      setPathError(err);
    } else {
      setPathError(null);
      setNewPath('');
    }
  };

  const filteredAudit = filterAuditEntries(sandbox.auditLog, auditFilter, auditQuery);

  return (
    <div className="flex-1 overflow-y-auto px-4 sm:px-8 py-8">
      <div className="max-w-5xl mx-auto space-y-8">
        <header className="flex flex-wrap items-end justify-between gap-4 pb-5 border-b border-hairline">
          <div className="space-y-1.5">
            <h1 className="font-editorial text-4xl text-ink">
              Landlock OS Sandbox & Network Shield
            </h1>
            <p className="text-sm text-body max-w-2xl leading-relaxed">
              Manage kernel-enforced filesystem path allowlists and SSRF egress controls. Sensitive system and credential directories are rejected fail-closed.
            </p>
          </div>

          <div className="flex items-center gap-2 text-xs font-medium text-success">
            <ShieldCheck className="w-4 h-4" />
            <span>Kernel Landlock {sandbox.landlockAbi} Active</span>
          </div>
        </header>

        <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
          {/* Path Allowlist Management */}
          <section className="lg:col-span-2 rounded-xl border border-hairline bg-canvas p-6 space-y-5">
            <div className="space-y-1">
              <h2 className="font-editorial text-2xl text-ink">
                Filesystem Allowlist Rules
              </h2>
              <p className="text-xs text-muted">
                Traversal segments (<code className="font-mono">..</code>), <code className="font-mono">/etc</code>, <code className="font-mono">/root</code>, and <code className="font-mono">.ssh</code> paths are automatically blocked.
              </p>
            </div>

            <form onSubmit={handleAdd} className="flex flex-wrap items-center gap-2.5">
              <input
                type="text"
                value={newPath}
                onChange={(e) => setNewPath(e.target.value)}
                placeholder="/workspace/docs or ~/Projects/shared"
                aria-label="Sandbox directory path"
                className="flex-1 min-w-[220px] h-10 rounded-md border border-hairline bg-canvas px-3 font-mono text-xs text-ink focus:outline-none focus:border-primary"
              />
              <select
                aria-label="Path permission mode"
                value={newMode}
                onChange={(e) => setNewMode(e.target.value as 'ro' | 'rw')}
                className="h-10 rounded-md border border-hairline bg-canvas px-3 text-xs font-medium text-ink focus:outline-none focus:border-primary"
              >
                <option value="ro">Read-Only (RO)</option>
                <option value="rw">Read-Write (RW)</option>
              </select>
              <button
                type="submit"
                className="h-10 px-4 inline-flex items-center gap-1.5 rounded-md bg-primary hover:bg-primary-active text-on-primary text-xs font-medium transition-colors whitespace-nowrap cursor-pointer"
              >
                <Plus className="w-3.5 h-3.5" />
                <span>Add Rule</span>
              </button>
            </form>

            {pathError && (
              <div
                role="alert"
                className="rounded-md border border-error bg-error/8 px-3.5 py-2.5 text-xs text-error"
              >
                {pathError}
              </div>
            )}

            <div className="divide-y divide-hairline-soft border-t border-hairline">
              {sandbox.paths.map((rule) => (
                <div
                  key={rule.id}
                  className="py-3 flex items-center justify-between gap-3 text-xs"
                >
                  <div className="min-w-0 space-y-0.5">
                    <div className="font-mono text-ink truncate">{rule.path}</div>
                    <div className="text-muted">Added: {rule.addedAt}</div>
                  </div>

                  <div className="flex items-center gap-3 shrink-0">
                    <span
                      className={`font-mono text-[11px] font-medium uppercase ${
                        rule.mode === 'rw' ? 'text-primary' : 'text-accent-teal'
                      }`}
                    >
                      {rule.mode}
                    </span>
                    <button
                      type="button"
                      onClick={() => onRemovePath(rule.id)}
                      aria-label={`Remove path ${rule.path}`}
                      className="h-8 w-8 inline-flex items-center justify-center rounded text-muted hover:text-error cursor-pointer"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </button>
                  </div>
                </div>
              ))}
            </div>
          </section>

          {/* Network & SSRF Shield */}
          <section className="rounded-xl border border-hairline bg-surface-card p-6 space-y-5">
            <div className="space-y-1">
              <h2 className="font-editorial text-2xl text-ink">
                Network Shield
              </h2>
              <p className="text-xs text-body">
                Controls outbound HTTP/HTTPS tool egress and blocks private RFC1918/metadata subnets.
              </p>
            </div>

            <div className="flex items-center justify-between gap-3 rounded-lg border border-hairline bg-canvas p-3.5">
              <div>
                <div className="text-xs font-medium text-ink">Outbound HTTP/HTTPS</div>
                <div className="text-[11px] text-muted">
                  {sandbox.outboundNetworkEnabled ? 'Permitted for tools' : 'Strict fail-closed'}
                </div>
              </div>
              <button
                type="button"
                onClick={() => onToggleOutbound(!sandbox.outboundNetworkEnabled)}
                className={`h-8 px-3 rounded-md text-xs font-medium transition-colors cursor-pointer ${
                  sandbox.outboundNetworkEnabled
                    ? 'bg-primary text-on-primary'
                    : 'bg-code-bg text-code-text'
                }`}
              >
                {sandbox.outboundNetworkEnabled ? 'Enabled' : 'Blocked'}
              </button>
            </div>

            <div className="space-y-2">
              <div className="text-xs font-medium text-body-strong">Blocked SSRF Subnets</div>
              <div className="rounded-lg bg-code-bg text-code-text p-3.5 space-y-1.5 font-mono text-xs">
                {sandbox.blockedCidrs.map((cidr) => (
                  <div key={cidr} className="flex items-center justify-between">
                    <span>{cidr}</span>
                    <span className="text-error">DENY</span>
                  </div>
                ))}
              </div>
            </div>
          </section>
        </div>

        {/* Security Audit Log */}
        <section className="rounded-xl border border-hairline bg-canvas p-6 space-y-4">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <h2 className="font-editorial text-2xl text-ink">
              Security Audit Stream
            </h2>

            <div className="flex flex-wrap items-center gap-2">
              <div className="inline-flex rounded-md border border-hairline bg-surface-soft p-0.5">
                {(['All', 'Allowed', 'Denied', 'Violation'] as const).map((tab) => (
                  <button
                    key={tab}
                    type="button"
                    onClick={() => setAuditFilter(tab)}
                    className={`px-3 py-1 rounded text-xs font-medium transition-colors cursor-pointer ${
                      auditFilter === tab
                        ? 'bg-canvas text-ink'
                        : 'text-muted hover:text-ink'
                    }`}
                  >
                    {tab}
                  </button>
                ))}
              </div>

              <div className="relative">
                <Search className="w-3.5 h-3.5 text-muted absolute left-2.5 top-1/2 -translate-y-1/2" />
                <input
                  type="text"
                  value={auditQuery}
                  onChange={(e) => setAuditQuery(e.target.value)}
                  placeholder="Filter target or actor..."
                  aria-label="Search audit log"
                  className="h-8 pl-8 pr-3 rounded-md border border-hairline bg-canvas text-xs text-ink focus:outline-none focus:border-primary"
                />
              </div>
            </div>
          </div>

          <div className="overflow-x-auto">
            <table className="w-full text-left border-collapse text-xs">
              <thead>
                <tr className="border-b border-hairline text-muted">
                  <th className="py-2.5 pr-4 font-medium">Time</th>
                  <th className="py-2.5 pr-4 font-medium">Actor</th>
                  <th className="py-2.5 pr-4 font-medium">Operation</th>
                  <th className="py-2.5 pr-4 font-medium">Target</th>
                  <th className="py-2.5 pr-4 font-medium">Outcome</th>
                  <th className="py-2.5 font-medium">Detail</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-hairline-soft">
                {filteredAudit.map((entry) => (
                  <tr key={entry.id}>
                    <td className="py-2.5 pr-4 font-mono text-muted tabular-nums whitespace-nowrap">
                      {entry.timestamp}
                    </td>
                    <td className="py-2.5 pr-4 font-medium text-ink whitespace-nowrap">
                      {entry.actor}
                    </td>
                    <td className="py-2.5 pr-4 font-mono text-body whitespace-nowrap">
                      {entry.operation}
                    </td>
                    <td className="py-2.5 pr-4 font-mono text-ink max-w-[200px] truncate">
                      {entry.target}
                    </td>
                    <td
                      className={`py-2.5 pr-4 font-medium whitespace-nowrap ${
                        entry.outcome === 'Allowed'
                          ? 'text-success'
                          : entry.outcome === 'Violation'
                          ? 'text-error'
                          : 'text-warning'
                      }`}
                    >
                      {entry.outcome}
                    </td>
                    <td className="py-2.5 text-body">{entry.detail}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
      </div>
    </div>
  );
}
