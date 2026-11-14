import React, { useEffect, useState } from 'react';
import { Check, FileCode2, ShieldAlert, X } from 'lucide-react';
import type { ApprovalRequest } from '../types/cockpit';

interface ApprovalCardProps {
  approval: ApprovalRequest;
  onResolve: (approvalId: string, decision: 'approved' | 'denied' | 'expired') => void;
  onInspectDiff?: () => void;
}

export function ApprovalCard({ approval, onResolve, onInspectDiff }: ApprovalCardProps) {
  const [secondsLeft, setSecondsLeft] = useState(approval.timeoutSeconds);
  const [submitting, setSubmitting] = useState(false);

  const isPending = approval.status === 'pending';

  useEffect(() => {
    if (!isPending) return;
    const interval = setInterval(() => {
      setSecondsLeft((prev) => {
        if (prev <= 1) {
          clearInterval(interval);
          onResolve(approval.id, 'expired');
          return 0;
        }
        return prev - 1;
      });
    }, 1000);
    return () => clearInterval(interval);
  }, [approval.id, isPending, onResolve]);

  useEffect(() => {
    if (!isPending) return;
    const handleKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (
        target &&
        (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.tagName === 'SELECT')
      ) {
        return;
      }
      if (e.key === 'y' || e.key === 'Y') {
        e.preventDefault();
        setSubmitting(true);
        onResolve(approval.id, 'approved');
      } else if (e.key === 'n' || e.key === 'N') {
        e.preventDefault();
        setSubmitting(true);
        onResolve(approval.id, 'denied');
      } else if ((e.key === 'e' || e.key === 'E') && onInspectDiff) {
        e.preventDefault();
        onInspectDiff();
      }
    };
    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  }, [approval.id, isPending, onInspectDiff, onResolve]);

  if (!isPending) {
    return (
      <div className="rounded-lg border border-hairline bg-surface-soft px-4 py-3 flex items-center justify-between gap-3 text-xs">
        <div className="flex items-center gap-2 min-w-0">
          <span
            className={`font-medium ${
              approval.status === 'approved' ? 'text-success' : 'text-error'
            }`}
          >
            {approval.status === 'approved'
              ? 'Approved'
              : approval.status === 'expired'
              ? 'Auto-denied (Timeout)'
              : 'Denied'}
          </span>
          <span className="text-muted-soft" aria-hidden="true">
            ·
          </span>
          <span className="text-body truncate">{approval.action}</span>
        </div>
        {approval.resolvedAt && (
          <span className="font-mono text-muted tabular-nums shrink-0">
            {approval.resolvedAt}
          </span>
        )}
      </div>
    );
  }

  const radius = 14;
  const circumference = 2 * Math.PI * radius;
  const progress = Math.max(0, secondsLeft / approval.timeoutSeconds);
  const strokeDashoffset = circumference * (1 - progress);
  const isUrgent = secondsLeft <= 10;

  return (
    <div
      role="region"
      aria-label="Supervised approval required"
      className="rounded-lg border border-primary bg-surface-card p-4 space-y-3.5 transition-colors"
    >
      <div className="flex items-start justify-between gap-4">
        <div className="flex items-start gap-2.5 min-w-0">
          <ShieldAlert className="w-4 h-4 text-primary mt-0.5 shrink-0" />
          <div className="min-w-0 space-y-0.5">
            <div className="text-xs font-medium uppercase tracking-[0.06em] text-primary-active">
              Supervised Approval Required · Fail-Closed Gate
            </div>
            <div className="text-sm font-medium text-ink">{approval.action}</div>
            <div className="font-mono text-xs text-muted truncate">{approval.target}</div>
          </div>
        </div>

        {/* Circular SVG Countdown Timer */}
        <div className="flex items-center gap-2 shrink-0">
          <svg className="w-9 h-9 -rotate-90" viewBox="0 0 36 36" aria-hidden="true">
            <circle
              cx="18"
              cy="18"
              r={radius}
              fill="none"
              stroke="var(--pc-hairline)"
              strokeWidth="3"
            />
            <circle
              cx="18"
              cy="18"
              r={radius}
              fill="none"
              stroke={isUrgent ? 'var(--pc-error)' : 'var(--pc-warning)'}
              strokeWidth="3"
              strokeDasharray={circumference}
              strokeDashoffset={strokeDashoffset}
              strokeLinecap="round"
              className="transition-all duration-1000 linear"
            />
          </svg>
          <span
            className={`font-mono text-xs tabular-nums ${
              isUrgent ? 'text-error font-medium' : 'text-body'
            }`}
          >
            Auto-deny {secondsLeft}s
          </span>
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-2.5">
        <button
          type="button"
          disabled={submitting}
          onClick={() => {
            setSubmitting(true);
            onResolve(approval.id, 'approved');
          }}
          className="h-10 px-4 inline-flex items-center gap-1.5 rounded-md bg-primary hover:bg-primary-active disabled:bg-primary-disabled text-on-primary text-xs font-medium transition-colors whitespace-nowrap cursor-pointer"
        >
          <Check className="w-3.5 h-3.5" />
          <span>Approve (Y)</span>
        </button>

        <button
          type="button"
          disabled={submitting}
          onClick={() => {
            setSubmitting(true);
            onResolve(approval.id, 'denied');
          }}
          className="h-10 px-4 inline-flex items-center gap-1.5 rounded-md border border-hairline bg-canvas hover:bg-surface-strong text-ink text-xs font-medium transition-colors whitespace-nowrap cursor-pointer"
        >
          <X className="w-3.5 h-3.5 text-error" />
          <span>Deny (N)</span>
        </button>

        {onInspectDiff && (
          <button
            type="button"
            onClick={onInspectDiff}
            className="h-10 px-3.5 inline-flex items-center gap-1.5 rounded-md border border-hairline bg-canvas hover:bg-surface-strong text-body text-xs font-medium transition-colors whitespace-nowrap cursor-pointer"
          >
            <FileCode2 className="w-3.5 h-3.5" />
            <span>Inspect Full Diff (E)</span>
          </button>
        )}
      </div>
    </div>
  );
}
