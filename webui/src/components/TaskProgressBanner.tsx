import React from 'react';
import { Check, ShieldAlert, Sparkles, XCircle } from 'lucide-react';
import { formatElapsedMs, type TaskProgressState } from '../lib/cockpitLogic';

interface TaskProgressBannerProps {
  progress: TaskProgressState;
  elapsedMs?: number;
  celebrating?: boolean;
}

function PawPrintSvg({
  className = '',
  fill = 'currentColor',
}: {
  className?: string;
  fill?: string;
}) {
  return (
    <svg
      viewBox="0 0 24 24"
      className={className}
      fill="none"
      aria-hidden="true"
    >
      {/* Main metacarpal pad */}
      <path
        d="M12 12.2c-2.6 0-4.7 2.2-4.7 4.4 0 1.6 1.2 2.9 2.7 2.9.8 0 1.4-.3 2-.3s1.2.3 2 .3c1.5 0 2.7-1.3 2.7-2.9 0-2.2-2.1-4.4-4.7-4.4Z"
        fill={fill}
      />
      {/* Four toe beans */}
      <ellipse cx="6.3" cy="10.8" rx="1.6" ry="2.1" transform="rotate(-18 6.3 10.8)" fill={fill} />
      <ellipse cx="10.0" cy="7.8" rx="1.55" ry="2.15" transform="rotate(-6 10 7.8)" fill={fill} />
      <ellipse cx="14.0" cy="7.8" rx="1.55" ry="2.15" transform="rotate(6 14 7.8)" fill={fill} />
      <ellipse cx="17.7" cy="10.8" rx="1.6" ry="2.1" transform="rotate(18 17.7 10.8)" fill={fill} />
    </svg>
  );
}

export function TaskProgressBanner({
  progress,
  elapsedMs = 0,
  celebrating = false,
}: TaskProgressBannerProps) {
  const isRunning = progress.status === 'running';
  const isWaiting = progress.status === 'awaiting-approval';
  const isCompleted = progress.status === 'completed';
  const isDenied = progress.status === 'denied';

  const barColorClass = isCompleted
    ? 'bg-success'
    : isDenied
    ? 'bg-error'
    : isWaiting
    ? 'bg-accent-amber'
    : 'bg-primary';

  const statusText = isCompleted
    ? 'Completed'
    : isDenied
    ? 'Halted'
    : isWaiting
    ? 'Awaiting approval'
    : 'In progress';

  const statusTextClass = isCompleted
    ? 'text-success'
    : isDenied
    ? 'text-error'
    : isWaiting
    ? 'text-accent-amber'
    : 'text-primary';

  // Generate staggered pawprint trail milestones up to the active percentage
  const trailSlots = [12, 26, 40, 54, 68, 82, 94].filter(
    (pos) => pos <= Math.max(progress.percentage - 4, 0)
  );

  return (
    <section
      aria-label="Multi-step AI task progress"
      data-testid="cat-paw-progress"
      className="shrink-0 border-b border-hairline bg-surface-soft px-4 sm:px-6 py-3 transition-colors"
    >
      <div className="max-w-4xl mx-auto space-y-2">
        {/* Top Metadata Row */}
        <div className="flex flex-wrap items-center justify-between gap-2 text-xs">
          <div className="flex items-center gap-2 min-w-0">
            <span className="inline-flex items-center gap-1.5 font-medium text-ink shrink-0">
              {isCompleted && celebrating ? (
                <Sparkles className="w-3.5 h-3.5 text-success animate-sparkle-pop" />
              ) : isCompleted ? (
                <Check className="w-3.5 h-3.5 text-success" />
              ) : isDenied ? (
                <XCircle className="w-3.5 h-3.5 text-error" />
              ) : isWaiting ? (
                <ShieldAlert className="w-3.5 h-3.5 text-accent-amber" />
              ) : (
                <PawPrintSvg className="w-3.5 h-3.5 text-primary animate-cat-paw-tap" />
              )}
              <span>{progress.activeActor} Task Progress</span>
            </span>
            <span className="text-muted-soft" aria-hidden="true">
              ·
            </span>
            <span className="text-body truncate">{progress.currentStepLabel}</span>
          </div>

          <div className="flex items-center gap-2.5 shrink-0 font-mono tabular-nums">
            {isRunning && elapsedMs > 0 && (
              <span className="text-[11px] text-muted" data-testid="task-elapsed-timer">
                {formatElapsedMs(elapsedMs)}
              </span>
            )}
            <span className="text-[11px] text-muted">
              Step {Math.min(progress.completedCount + (isCompleted ? 0 : 1), progress.totalSteps)}/{progress.totalSteps}
            </span>
            <span className={`text-[11px] font-medium ${statusTextClass}`}>{statusText}</span>
            <span className="text-xs font-semibold text-ink">{progress.percentage}%</span>
          </div>
        </div>

        {/* Cat-Paw-Walking Progress Track */}
        <div
          role="progressbar"
          aria-label="Multi-step AI task progress"
          aria-valuenow={progress.percentage}
          aria-valuemin={0}
          aria-valuemax={100}
          className="relative h-6 flex items-center"
        >
          {/* Base Track */}
          <div className="relative w-full h-2.5 rounded-full bg-surface-strong border border-hairline overflow-hidden">
            {/* Filled Progress Bar */}
            <div
              className={`h-full rounded-full transition-[width] duration-500 ease-out ${barColorClass}`}
              style={{ width: `${progress.percentage}%` }}
            />

            {/* Subtle Staggered Pawprint Trail inside completed portion */}
            <div className="absolute inset-0 pointer-events-none">
              {trailSlots.map((slot, idx) => (
                <span
                  key={slot}
                  className={`absolute top-1/2 -translate-x-1/2 ${
                    idx % 2 === 0 ? '-translate-y-[62%] -rotate-12' : '-translate-y-[38%] rotate-12'
                  } ${isRunning ? 'animate-cat-paw-trail' : ''}`}
                  style={{
                    left: `${slot}%`,
                    animationDelay: `${idx * 140}ms`,
                  }}
                >
                  <PawPrintSvg
                    className="w-2.5 h-2.5 text-on-primary/80"
                    fill="currentColor"
                  />
                </span>
              ))}
            </div>
          </div>

          {/* Leading-Edge Cat-Paw-Walking Walker */}
          <div
            data-testid="cat-paw-walker"
            data-walking={isRunning || isWaiting ? 'true' : 'false'}
            className="absolute top-1/2 -translate-y-1/2 -translate-x-1/2 pointer-events-none transition-[left] duration-500 ease-out z-10"
            style={{
              left: `clamp(16px, ${progress.percentage}%, calc(100% - 16px))`,
            }}
          >
            <div
              className={`flex items-center justify-center rounded-full border border-hairline bg-canvas px-1.5 py-0.5 shadow-xs ${statusTextClass}`}
            >
              {/* Alternating Left & Right Cat Paws */}
              <div className="flex items-center gap-0.5">
                <span
                  className={`inline-block ${
                    isRunning
                      ? 'animate-cat-paw-walking-left'
                      : isWaiting
                      ? 'animate-cat-paw-tap'
                      : ''
                  }`}
                >
                  <PawPrintSvg className="w-3.5 h-3.5 rotate-90" fill="currentColor" />
                </span>
                <span
                  className={`inline-block ${
                    isRunning
                      ? 'animate-cat-paw-walking-right'
                      : ''
                  }`}
                >
                  <PawPrintSvg className="w-3 h-3 rotate-90 opacity-85" fill="currentColor" />
                </span>
              </div>
            </div>
          </div>
        </div>

        {/* Compact Unboxed Step Breadcrumbs */}
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] font-mono text-muted">
          {progress.steps.map((step, index) => {
            const isDone = step.status === 'completed';
            const isActive = step.status === 'running' || step.status === 'pending';
            const isStepDenied = step.status === 'denied';

            return (
              <span
                key={step.id}
                className={`inline-flex items-center gap-1 min-w-0 max-w-[220px] ${
                  isDone
                    ? 'text-body'
                    : isStepDenied
                    ? 'text-error'
                    : isActive
                    ? 'text-ink font-medium'
                    : 'text-muted'
                }`}
              >
                <span
                  className={`w-1.5 h-1.5 rounded-full shrink-0 ${
                    isDone
                      ? 'bg-success'
                      : isStepDenied
                      ? 'bg-error'
                      : step.status === 'running'
                      ? 'bg-primary animate-paw-step'
                      : 'bg-accent-amber'
                  }`}
                />
                <span className="truncate">
                  {index + 1}. {step.label}
                </span>
              </span>
            );
          })}
        </div>
      </div>
    </section>
  );
}
