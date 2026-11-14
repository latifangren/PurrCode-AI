import React from 'react';
import { DEFAULT_PREFERENCES } from '../themes/themeRegistry';
import { CatCompanionSvg } from './CatCompanionSvg';

function ShimmerBar({ className = '' }: { className?: string }) {
  return (
    <div
      className={`relative overflow-hidden rounded bg-surface-strong ${className}`}
      aria-hidden="true"
    >
      <div className="absolute inset-0 bg-gradient-to-r from-transparent via-canvas/45 to-transparent animate-skeleton-shimmer" />
    </div>
  );
}

export function CockpitBootSkeleton() {
  return (
    <div
      aria-busy="true"
      aria-label="Loading PurrCode Cockpit"
      className="h-screen w-screen overflow-hidden flex flex-col bg-canvas text-ink"
    >
      {/* TopBar skeleton */}
      <div className="h-14 shrink-0 border-b border-hairline px-5 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <ShimmerBar className="h-8 w-8 rounded-md" />
          <ShimmerBar className="h-5 w-28" />
        </div>
        <div className="flex items-center gap-2">
          <ShimmerBar className="h-9 w-24 rounded-md" />
          <ShimmerBar className="h-9 w-28 rounded-md" />
        </div>
      </div>

      {/* Tri-pane skeleton */}
      <div className="flex-1 min-h-0 flex">
        <div className="hidden lg:flex w-[264px] shrink-0 border-r border-hairline bg-surface-soft p-4 flex-col gap-3">
          <ShimmerBar className="h-4 w-24" />
          <ShimmerBar className="h-9 w-full rounded-md" />
          <ShimmerBar className="h-9 w-full rounded-md" />
          <ShimmerBar className="h-9 w-full rounded-md" />
          <ShimmerBar className="h-9 w-full rounded-md" />
        </div>

        <div className="flex-1 flex flex-col justify-between p-6 max-w-4xl mx-auto w-full">
          <div className="space-y-5">
            <div className="flex items-center gap-3 pb-3 border-b border-hairline">
              <CatCompanionSvg
                companion={DEFAULT_PREFERENCES.companion}
                mood="thinking"
                size="sm"
              />
              <div className="space-y-1">
                <div className="text-sm font-medium text-ink">
                  Waking up PurrCode Cockpit
                </div>
                <div className="text-xs text-muted">
                  Syncing Landlock policy, Clowder routing, and session state...
                </div>
              </div>
            </div>

            <div className="rounded-lg border border-hairline bg-surface-card p-5 space-y-3">
              <ShimmerBar className="h-4 w-32" />
              <ShimmerBar className="h-4 w-full" />
              <ShimmerBar className="h-4 w-3/4" />
            </div>

            <div className="rounded-lg border border-hairline bg-canvas p-5 space-y-3">
              <ShimmerBar className="h-4 w-44" />
              <ShimmerBar className="h-10 w-full rounded-md" />
              <ShimmerBar className="h-4 w-5/6" />
            </div>
          </div>

          <div className="rounded-lg border border-hairline p-4 space-y-2">
            <ShimmerBar className="h-10 w-full" />
          </div>
        </div>

        <div className="hidden xl:flex w-[296px] shrink-0 border-l border-hairline bg-surface-soft p-4 flex-col gap-4">
          <ShimmerBar className="h-24 w-full rounded-lg" />
          <ShimmerBar className="h-12 w-full" />
          <ShimmerBar className="h-28 w-full" />
        </div>
      </div>
    </div>
  );
}

export function ModuleViewSkeleton() {
  return (
    <div
      aria-busy="true"
      aria-label="Loading module view"
      className="flex-1 overflow-y-auto px-4 sm:px-8 py-8"
    >
      <div className="max-w-5xl mx-auto space-y-6">
        <div className="pb-5 border-b border-hairline space-y-2">
          <ShimmerBar className="h-9 w-64" />
          <ShimmerBar className="h-4 w-96 max-w-full" />
        </div>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-5">
          <div className="rounded-xl border border-hairline bg-surface-card p-6 space-y-3">
            <ShimmerBar className="h-5 w-40" />
            <ShimmerBar className="h-14 w-full rounded-md" />
            <ShimmerBar className="h-8 w-28" />
          </div>
          <div className="rounded-xl border border-hairline bg-surface-card p-6 space-y-3">
            <ShimmerBar className="h-5 w-40" />
            <ShimmerBar className="h-14 w-full rounded-md" />
            <ShimmerBar className="h-8 w-28" />
          </div>
        </div>
      </div>
    </div>
  );
}
