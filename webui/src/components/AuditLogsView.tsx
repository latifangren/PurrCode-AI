import React, { useCallback, useEffect, useMemo, useState } from 'react';
import {
  AlertTriangle,
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  RefreshCw,
  Search,
  Shield,
  ShieldAlert,
  XCircle,
} from 'lucide-react';
import type { AgentRole, AuditEntry, AuditOutcome } from '../types/cockpit';

interface AuditLogsViewProps {
  initialAuditLog: AuditEntry[];
}

const OUTCOMES: ('All' | AuditOutcome)[] = [
  'All',
  'Allowed',
  'Blocked',
  'Denied',
  'Violation',
];

const ACTORS: ('All' | AgentRole | 'System')[] = [
  'All',
  'Meowstro',
  'Tinker',
  'Whisker',
  'Sphinx',
  'Calico',
  'Tuxeshadow',
  'System',
];

export function AuditLogsView({ initialAuditLog }: AuditLogsViewProps) {
  const [entries, setEntries] = useState<AuditEntry[]>(initialAuditLog);
  const [outcomeFilter, setOutcomeFilter] = useState<'All' | AuditOutcome>('All');
  const [actorFilter, setActorFilter] = useState<'All' | AgentRole | 'System'>('All');
  const [searchQuery, setSearchQuery] = useState('');
  const [autoRefresh, setAutoRefresh] = useState(true);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [expandedId, setExpandedId] = useState<string | null>(null);

  const fetchLogs = useCallback(async () => {
    setIsRefreshing(true);
    try {
      const resp = await fetch('/api/logs');
      if (resp.ok) {
        const data = (await resp.json()) as { entries?: AuditEntry[] };
        if (data.entries && Array.isArray(data.entries)) {
          setEntries(data.entries);
        }
      }
    } catch {
      // keep existing entries if fetch fails
    } finally {
      setIsRefreshing(false);
    }
  }, []);

  useEffect(() => {
    if (!autoRefresh) return;
    const interval = setInterval(() => {
      void fetchLogs();
    }, 5000);
    return () => clearInterval(interval);
  }, [autoRefresh, fetchLogs]);

  const filteredEntries = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    return entries.filter((entry) => {
      if (outcomeFilter !== 'All' && entry.outcome !== outcomeFilter) {
        return false;
      }
      if (actorFilter !== 'All' && entry.actor !== actorFilter) {
        return false;
      }
      if (!q) return true;
      return (
        entry.id.toLowerCase().includes(q) ||
        entry.actor.toLowerCase().includes(q) ||
        entry.operation.toLowerCase().includes(q) ||
        entry.target.toLowerCase().includes(q) ||
        entry.detail.toLowerCase().includes(q)
      );
    });
  }, [entries, outcomeFilter, actorFilter, searchQuery]);

  const getOutcomeBadge = (outcome: AuditOutcome) => {
    switch (outcome) {
      case 'Allowed':
        return (
          <span className="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full text-xs font-mono font-medium bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border border-emerald-500/20">
            <CheckCircle2 className="w-3.5 h-3.5" />
            Allowed
          </span>
        );
      case 'Blocked':
        return (
          <span className="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full text-xs font-mono font-medium bg-amber-500/10 text-amber-600 dark:text-amber-400 border border-amber-500/20">
            <AlertTriangle className="w-3.5 h-3.5" />
            Blocked
          </span>
        );
      case 'Denied':
        return (
          <span className="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full text-xs font-mono font-medium bg-rose-500/10 text-rose-600 dark:text-rose-400 border border-rose-500/20">
            <XCircle className="w-3.5 h-3.5" />
            Denied
          </span>
        );
      case 'Violation':
        return (
          <span className="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full text-xs font-mono font-medium bg-purple-500/10 text-purple-600 dark:text-purple-400 border border-purple-500/20">
            <ShieldAlert className="w-3.5 h-3.5" />
            Violation
          </span>
        );
    }
  };

  const getActorBadge = (actor: string) => {
    let color = 'bg-ink/5 text-ink border-hairline';
    if (actor === 'Meowstro') color = 'bg-coral/10 text-coral border-coral/30';
    if (actor === 'Tinker') color = 'bg-amber-500/10 text-amber-500 border-amber-500/30';
    if (actor === 'Whisker') color = 'bg-sky-500/10 text-sky-500 border-sky-500/30';
    if (actor === 'Sphinx') color = 'bg-purple-500/10 text-purple-500 border-purple-500/30';
    if (actor === 'Calico') color = 'bg-pink-500/10 text-pink-500 border-pink-500/30';
    if (actor === 'Tuxeshadow') color = 'bg-emerald-500/10 text-emerald-500 border-emerald-500/30';

    return (
      <span className={`inline-flex items-center px-2 py-0.5 rounded text-xs font-mono font-semibold border ${color}`}>
        {actor}
      </span>
    );
  };

  return (
    <div className="flex-1 overflow-y-auto px-4 sm:px-8 py-8">
      <div className="max-w-6xl mx-auto space-y-6">
        {/* Header */}
        <header className="flex flex-wrap items-end justify-between gap-4 pb-5 border-b border-hairline">
          <div className="space-y-1.5">
            <div className="flex items-center gap-2.5">
              <Shield className="w-7 h-7 text-coral" />
              <h1 className="font-editorial text-4xl text-ink">
                Audit & Security Logs
              </h1>
            </div>
            <p className="text-sm text-body max-w-2xl leading-relaxed">
              Real-time audit trail recording every agent operation, tool call, sandbox boundary check, and control plane mutation.
            </p>
          </div>

          <div className="flex items-center gap-3">
            <span className="text-xs font-mono text-muted">
              {filteredEntries.length} of {entries.length} events
            </span>
            <button
              type="button"
              onClick={() => setAutoRefresh((v) => !v)}
              className={`px-3 py-1.5 rounded-lg text-xs font-mono transition-colors border ${
                autoRefresh
                  ? 'bg-coral/10 border-coral text-coral'
                  : 'bg-card border-hairline text-muted hover:text-ink'
              }`}
            >
              Auto-refresh: {autoRefresh ? 'ON' : 'OFF'}
            </button>
            <button
              type="button"
              onClick={() => void fetchLogs()}
              disabled={isRefreshing}
              className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-mono bg-card hover:bg-card-hover border border-hairline text-ink transition-colors disabled:opacity-50"
            >
              <RefreshCw className={`w-3.5 h-3.5 ${isRefreshing ? 'animate-spin' : ''}`} />
              Refresh
            </button>
          </div>
        </header>

        {/* Filter Toolbar */}
        <div className="bg-card border border-hairline rounded-xl p-4 space-y-3 shadow-subtle">
          <div className="flex flex-col sm:flex-row gap-3">
            {/* Search Input */}
            <div className="relative flex-1">
              <Search className="w-4 h-4 absolute left-3 top-1/2 -translate-y-1/2 text-muted" />
              <input
                type="text"
                placeholder="Search audit trail by actor, operation, target, or details..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="w-full pl-9 pr-3 py-2 text-xs font-mono rounded-lg bg-canvas border border-hairline focus:border-coral focus:outline-none text-ink placeholder:text-muted"
              />
            </div>

            {/* Actor Filter Dropdown */}
            <div className="flex items-center gap-2">
              <label htmlFor="actor-filter-select" className="text-xs font-mono text-muted whitespace-nowrap">Actor:</label>
              <select
                id="actor-filter-select"
                aria-label="Filter logs by actor"
                value={actorFilter}
                onChange={(e) => setActorFilter(e.target.value as 'All' | AgentRole | 'System')}
                className="px-3 py-2 text-xs font-mono rounded-lg bg-canvas border border-hairline focus:border-coral focus:outline-none text-ink cursor-pointer"
              >
                {ACTORS.map((act) => (
                  <option key={act} value={act}>
                    {act}
                  </option>
                ))}
              </select>
            </div>
          </div>

          {/* Outcome Filter Buttons */}
          <div className="flex flex-wrap items-center gap-1.5 pt-1 border-t border-hairline/50">
            <span className="text-xs font-mono text-muted mr-1">Outcome:</span>
            {OUTCOMES.map((out) => (
              <button
                key={out}
                type="button"
                onClick={() => setOutcomeFilter(out)}
                className={`px-2.5 py-1 text-xs font-mono rounded-md transition-colors ${
                  outcomeFilter === out
                    ? 'bg-coral text-white font-medium shadow-sm'
                    : 'bg-canvas text-muted hover:text-ink border border-hairline'
                }`}
              >
                {out}
              </button>
            ))}
          </div>
        </div>

        {/* Audit Log Table */}
        <div className="bg-card border border-hairline rounded-xl overflow-hidden shadow-subtle">
          {filteredEntries.length === 0 ? (
            <div className="py-16 text-center text-muted space-y-2">
              <Shield className="w-8 h-8 mx-auto stroke-1 text-muted/60" />
              <p className="text-sm font-mono">No audit log entries matching filters</p>
            </div>
          ) : (
            <div className="overflow-x-auto">
              <table className="w-full text-left text-xs font-mono">
                <thead>
                  <tr className="border-b border-hairline bg-canvas/60 text-muted uppercase tracking-wider text-[11px]">
                    <th className="py-3 px-4 font-semibold">Timestamp</th>
                    <th className="py-3 px-4 font-semibold">Actor</th>
                    <th className="py-3 px-4 font-semibold">Operation</th>
                    <th className="py-3 px-4 font-semibold">Target</th>
                    <th className="py-3 px-4 font-semibold">Outcome</th>
                    <th className="py-3 px-4 font-semibold text-right">Details</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-hairline">
                  {filteredEntries.map((entry) => {
                    const isExpanded = expandedId === entry.id;
                    return (
                      <React.Fragment key={entry.id}>
                        <tr
                          onClick={() => setExpandedId(isExpanded ? null : entry.id)}
                          className="hover:bg-canvas/50 transition-colors cursor-pointer group"
                        >
                          <td className="py-3 px-4 text-muted whitespace-nowrap">
                            {entry.timestamp}
                          </td>
                          <td className="py-3 px-4 whitespace-nowrap">
                            {getActorBadge(entry.actor)}
                          </td>
                          <td className="py-3 px-4 font-semibold text-ink whitespace-nowrap">
                            {entry.operation}
                          </td>
                          <td className="py-3 px-4 text-body max-w-xs truncate">
                            <span className="px-1.5 py-0.5 rounded bg-canvas border border-hairline text-[11px]">
                              {entry.target}
                            </span>
                          </td>
                          <td className="py-3 px-4 whitespace-nowrap">
                            {getOutcomeBadge(entry.outcome)}
                          </td>
                          <td className="py-3 px-4 text-right whitespace-nowrap text-muted group-hover:text-coral transition-colors">
                            <span className="inline-flex items-center gap-1">
                              {isExpanded ? (
                                <>
                                  Hide <ChevronUp className="w-3.5 h-3.5" />
                                </>
                              ) : (
                                <>
                                  View <ChevronDown className="w-3.5 h-3.5" />
                                </>
                              )}
                            </span>
                          </td>
                        </tr>
                        {isExpanded && (
                          <tr className="bg-canvas/40 border-b border-hairline">
                            <td colSpan={6} className="p-4 space-y-2">
                              <div className="flex items-center justify-between text-[11px] text-muted">
                                <span>Entry ID: <code className="text-ink">{entry.id}</code></span>
                                <span>Operation: <code className="text-ink">{entry.operation}</code></span>
                              </div>
                              <div className="p-3 bg-card border border-hairline rounded-lg text-ink font-mono text-xs whitespace-pre-wrap break-all">
                                {entry.detail || 'No additional details recorded.'}
                              </div>
                            </td>
                          </tr>
                        )}
                      </React.Fragment>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
