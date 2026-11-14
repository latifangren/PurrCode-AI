import React from 'react';
import { AlertCircle, CheckCircle2, Info, ShieldAlert, X } from 'lucide-react';
import type { ToastItem, ToastTone } from '../types/cockpit';

interface ToastRegionProps {
  toasts: ToastItem[];
  onDismiss: (id: string) => void;
}

function ToneIcon({ tone }: { tone: ToastTone }) {
  switch (tone) {
    case 'success':
      return <CheckCircle2 className="w-4 h-4 text-success shrink-0" />;
    case 'warning':
      return <ShieldAlert className="w-4 h-4 text-warning shrink-0" />;
    case 'error':
      return <AlertCircle className="w-4 h-4 text-error shrink-0" />;
    default:
      return <Info className="w-4 h-4 text-primary shrink-0" />;
  }
}

export function ToastRegion({ toasts, onDismiss }: ToastRegionProps) {
  if (toasts.length === 0) return null;

  return (
    <div
      aria-live="polite"
      aria-label="Cockpit notifications"
      className="fixed top-16 right-4 z-50 flex flex-col gap-2 w-[calc(100vw-2rem)] max-w-xs pointer-events-none"
    >
      {toasts.map((toast) => (
        <div
          key={toast.id}
          role="status"
          className="pointer-events-auto rounded-lg border border-hairline bg-canvas px-3.5 py-2.5 shadow-sm flex items-start justify-between gap-2.5 text-xs transition-all"
        >
          <div className="flex items-start gap-2 min-w-0">
            <div className="mt-0.5">
              <ToneIcon tone={toast.tone} />
            </div>
            <div className="min-w-0 space-y-0.5">
              <div className="font-medium text-ink truncate">{toast.title}</div>
              {toast.detail && (
                <div className="text-muted text-[11px] leading-snug break-words">
                  {toast.detail}
                </div>
              )}
            </div>
          </div>
          <button
            type="button"
            onClick={() => onDismiss(toast.id)}
            aria-label={`Dismiss ${toast.title}`}
            className="h-6 w-6 shrink-0 inline-flex items-center justify-center rounded text-muted hover:text-ink cursor-pointer"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        </div>
      ))}
    </div>
  );
}
