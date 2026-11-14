import React from 'react';
import { X } from 'lucide-react';
import {
  computeContextMetrics,
  pickCompanionReactionQuip,
} from '../lib/cockpitLogic';
import type {
  CompanionConfig,
  CompanionMood,
  CompanionReaction,
  SandboxState,
  Session,
  SubagentRouting,
} from '../types/cockpit';
import { CatCompanionSvg } from './CatCompanionSvg';

interface InspectorHudProps {
  open: boolean;
  session: Session;
  contextLimit: number;
  routing: SubagentRouting[];
  sandbox: SandboxState;
  companion: CompanionConfig;
  companionMood: CompanionMood;
  companionReaction: CompanionReaction;
  onPetCompanion: () => void;
  onClose: () => void;
  onOpenSandbox: () => void;
  onOpenAppearance: () => void;
}

const MOOD_LABEL: Record<CompanionMood, string> = {
  idle: 'Resting · Watching workspace',
  thinking: 'Working · Streaming response',
  approval: 'Alert · Approval required',
  excited: 'Oyen Mode · Full speed',
};

export function InspectorHud({
  open,
  session,
  contextLimit,
  routing,
  sandbox,
  companion,
  companionMood,
  companionReaction,
  onPetCompanion,
  onClose,
  onOpenSandbox,
  onOpenAppearance,
}: InspectorHudProps) {
  if (!open) return null;

  const metrics = computeContextMetrics(session.messages, contextLimit);
  const roCount = sandbox.paths.filter((p) => p.mode === 'ro').length;
  const rwCount = sandbox.paths.filter((p) => p.mode === 'rw').length;
  const recentAudit = sandbox.auditLog.slice(0, 5);
  const reactionQuip = pickCompanionReactionQuip(companionReaction, companion.name);

  return (
    <aside
      aria-label="Live telemetry inspector"
      className="hidden xl:flex w-[296px] shrink-0 border-l border-hairline bg-surface-soft flex-col overflow-y-auto transition-colors"
    >
      <div className="flex items-center justify-between px-4 py-3.5 border-b border-hairline">
        <span className="text-xs font-medium tracking-[0.06em] uppercase text-muted">
          Live Inspector
        </span>
        <button
          type="button"
          onClick={onClose}
          aria-label="Close inspector panel"
          className="h-7 w-7 inline-flex items-center justify-center rounded text-muted hover:text-ink cursor-pointer"
        >
          <X className="w-3.5 h-3.5" />
        </button>
      </div>

      <div className="p-4 space-y-6 text-sm">
        {/* Interactive Cat Companion Stage */}
        {companion.visibility === 'full' && (
          <section className="rounded-lg border border-hairline bg-surface-card p-3.5 space-y-2.5">
            <div className="flex items-center justify-between gap-2">
              <span className="text-xs font-medium text-ink">{companion.name}</span>
              <button
                type="button"
                onClick={onOpenAppearance}
                className="text-[11px] font-medium text-primary hover:text-primary-active cursor-pointer"
              >
                Customize
              </button>
            </div>
            <div className="flex items-center gap-3">
              <button
                type="button"
                onClick={onPetCompanion}
                aria-label={`Pet companion ${companion.name}`}
                title={`Click to pet ${companion.name}`}
                className="shrink-0 rounded-lg p-1 hover:bg-surface-strong/70 transition-colors cursor-pointer"
              >
                <CatCompanionSvg
                  companion={companion}
                  mood={companionMood}
                  reaction={companionReaction}
                  size="md"
                />
              </button>
              <div className="min-w-0 space-y-1">
                <div className="text-xs font-medium text-body-strong">
                  {MOOD_LABEL[companionMood]}
                </div>
                <p className="text-[11px] text-muted leading-snug">
                  {reactionQuip ?? companion.tagline}
                </p>
              </div>
            </div>
          </section>
        )}

        {/* Context Window Usage */}
        <section className="space-y-2">
          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-body-strong">Context Window</span>
            <span className="font-mono text-xs text-muted tabular-nums">
              {metrics.percentUsed}% used
            </span>
          </div>
          <div className="h-2 w-full rounded-sm bg-hairline overflow-hidden">
            <div
              className={`h-full transition-all duration-200 ${
                metrics.status === 'critical'
                  ? 'bg-error'
                  : metrics.status === 'elevated'
                  ? 'bg-warning'
                  : 'bg-primary'
              }`}
              style={{ width: `${Math.max(4, metrics.percentUsed)}%` }}
            />
          </div>
          <div className="flex items-center justify-between font-mono text-[11px] text-muted tabular-nums">
            <span>{metrics.usedTokens.toLocaleString()} tokens</span>
            <span>{(metrics.limitTokens / 1000).toFixed(0)}k max</span>
          </div>
        </section>

        {/* Subagent Hierarchy Stack */}
        <section className="space-y-2.5 pt-4 border-t border-hairline">
          <div className="text-xs font-medium text-body-strong">Subagent Hierarchy</div>
          <div className="space-y-1.5 font-mono text-xs">
            {routing.slice(0, 4).map((agent, idx) => (
              <div
                key={agent.role}
                className="flex items-center justify-between py-1"
                style={{ paddingLeft: `${idx === 0 ? 0 : 12}px` }}
              >
                <span className="text-ink truncate">
                  {idx === 0 ? agent.role : `└ ${agent.role}`}
                </span>
                <span
                  className={`text-[11px] ${
                    agent.status === 'active' ? 'text-primary font-medium' : 'text-muted-soft'
                  }`}
                >
                  {agent.status === 'active' ? 'Running' : 'Standby'}
                </span>
              </div>
            ))}
          </div>
        </section>

        {/* Landlock Policy Summary */}
        <section className="space-y-2.5 pt-4 border-t border-hairline">
          <div className="flex items-center justify-between">
            <span className="text-xs font-medium text-body-strong">Landlock Sandbox</span>
            <button
              type="button"
              onClick={onOpenSandbox}
              className="text-xs font-medium text-primary hover:text-primary-active cursor-pointer"
            >
              Configure
            </button>
          </div>
          <dl className="grid grid-cols-2 gap-y-1.5 text-xs">
            <dt className="text-muted">Kernel ABI</dt>
            <dd className="text-right font-mono text-ink">
              {sandbox.landlockAbi} · {sandbox.enforced ? 'Enforced' : 'Off'}
            </dd>
            <dt className="text-muted">Allowlisted Paths</dt>
            <dd className="text-right font-mono text-ink tabular-nums">
              {roCount} RO · {rwCount} RW
            </dd>
            <dt className="text-muted">Outbound Net</dt>
            <dd className="text-right font-mono text-ink">
              {sandbox.outboundNetworkEnabled ? 'Permitted' : 'Strict Blocked'}
            </dd>
          </dl>
        </section>

        {/* Recent Audit Log */}
        <section className="space-y-2.5 pt-4 border-t border-hairline">
          <div className="text-xs font-medium text-body-strong">Recent Audit Stream</div>
          <div className="space-y-2">
            {recentAudit.map((item) => (
              <div
                key={item.id}
                className="rounded-md bg-canvas border border-hairline px-2.5 py-2 space-y-0.5"
              >
                <div className="flex items-center justify-between text-[11px] font-mono">
                  <span className="text-muted tabular-nums">{item.timestamp}</span>
                  <span
                    className={
                      item.outcome === 'Allowed'
                        ? 'text-success'
                        : item.outcome === 'Violation'
                        ? 'text-error font-medium'
                        : 'text-warning'
                    }
                  >
                    {item.outcome}
                  </span>
                </div>
                <div className="text-xs text-ink font-medium truncate">
                  {item.actor} · {item.operation}
                </div>
                <div className="font-mono text-[11px] text-muted truncate">
                  {item.target}
                </div>
              </div>
            ))}
          </div>
        </section>
      </div>
    </aside>
  );
}
