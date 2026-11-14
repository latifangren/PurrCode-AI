import React, { useState } from 'react';
import { ChevronDown, ChevronRight, FileText, Pencil, Search, Terminal } from 'lucide-react';
import type { ToolStep } from '../types/cockpit';

interface ToolChainProps {
  steps: ToolStep[];
}

function ToolIcon({ tool }: { tool: ToolStep['tool'] }) {
  switch (tool) {
    case 'read':
      return <FileText className="w-3.5 h-3.5 text-accent-teal shrink-0" />;
    case 'grep':
      return <Search className="w-3.5 h-3.5 text-accent-amber shrink-0" />;
    case 'edit':
      return <Pencil className="w-3.5 h-3.5 text-primary shrink-0" />;
    default:
      return <Terminal className="w-3.5 h-3.5 text-code-muted shrink-0" />;
  }
}

export function ToolChain({ steps }: ToolChainProps) {
  const [expanded, setExpanded] = useState(false);

  if (steps.length === 0) return null;

  const totalMs = steps.reduce((acc, s) => acc + s.durationMs, 0);
  const hasRunning = steps.some((s) => s.status === 'running');
  const hasPending = steps.some((s) => s.status === 'pending');
  const hasDenied = steps.some((s) => s.status === 'denied');

  const summaryStatus = hasDenied
    ? 'Blocked'
    : hasRunning
    ? 'Running'
    : hasPending
    ? 'Awaiting approval'
    : 'Completed';

  return (
    <div className="rounded-lg border border-hairline bg-surface-soft overflow-hidden">
      <button
        type="button"
        onClick={() => setExpanded((prev) => !prev)}
        aria-expanded={expanded}
        className="w-full px-3.5 py-2.5 flex items-center justify-between gap-3 text-left hover:bg-surface-card transition-colors cursor-pointer"
      >
        <div className="flex items-center gap-2 min-w-0">
          {expanded ? (
            <ChevronDown className="w-4 h-4 text-muted shrink-0" />
          ) : (
            <ChevronRight className="w-4 h-4 text-muted shrink-0" />
          )}
          <span className="text-xs font-medium text-ink truncate">
            {summaryStatus} · Ran {steps.length} tool {steps.length === 1 ? 'operation' : 'operations'}
          </span>
        </div>
        <span className="font-mono text-[11px] text-muted tabular-nums shrink-0">
          {totalMs}ms
        </span>
      </button>

      {(hasRunning || hasPending) && (
        <div className="h-[2px] w-full bg-hairline overflow-hidden">
          <div className="h-full w-1/3 bg-primary animate-scan-bar" />
        </div>
      )}

      {expanded && (
        <div className="border-t border-hairline bg-code-bg text-code-text px-4 py-3 space-y-2.5">
          {steps.map((step) => (
            <div key={step.id} className="flex items-start justify-between gap-4 text-xs">
              <div className="flex items-start gap-2.5 min-w-0">
                <div className="mt-0.5">
                  <ToolIcon tool={step.tool} />
                </div>
                <div className="min-w-0 space-y-0.5">
                  <div className="font-mono text-code-text truncate">
                    <span className="text-code-muted mr-1.5">{step.tool}</span>
                    {step.target}
                  </div>
                  <div className="text-code-muted text-[12px]">{step.summary}</div>
                </div>
              </div>
              <div className="flex items-center gap-2.5 shrink-0 font-mono text-[11px] tabular-nums">
                <span
                  className={
                    step.status === 'completed'
                      ? 'text-success'
                      : step.status === 'denied'
                      ? 'text-error'
                      : 'text-accent-amber'
                  }
                >
                  {step.status}
                </span>
                <span className="text-muted-soft">{step.durationMs}ms</span>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
