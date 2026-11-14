import React from 'react';
import { Maximize2 } from 'lucide-react';
import type { UnifiedDiff } from '../types/cockpit';

interface DiffViewerProps {
  diff: UnifiedDiff;
  onInspectFull?: () => void;
}

export function DiffViewer({ diff, onInspectFull }: DiffViewerProps) {
  return (
    <div className="rounded-lg bg-code-bg text-code-text overflow-hidden border border-code-border">
      <div className="px-4 py-2.5 bg-code-elevated flex items-center justify-between gap-3">
        <div className="flex items-center gap-2 min-w-0">
          <span className="font-mono text-xs text-code-text truncate">{diff.filePath}</span>
          <span className="font-mono text-[11px] text-code-muted">· {diff.language}</span>
        </div>

        <div className="flex items-center gap-3 shrink-0">
          <span
            className={`font-mono text-[11px] ${
              diff.status === 'accepted'
                ? 'text-success'
                : diff.status === 'rejected'
                ? 'text-error'
                : 'text-accent-amber'
            }`}
          >
            {diff.status === 'accepted'
              ? 'Applied'
              : diff.status === 'rejected'
              ? 'Rejected'
              : 'Staged'}
          </span>
          {onInspectFull && (
            <button
              type="button"
              onClick={onInspectFull}
              aria-label={`Inspect full diff for ${diff.filePath}`}
              className="inline-flex items-center gap-1 text-xs text-code-muted hover:text-code-text transition-colors cursor-pointer"
            >
              <Maximize2 className="w-3.5 h-3.5" />
              <span>Inspect</span>
            </button>
          )}
        </div>
      </div>

      <div className="bg-code-soft overflow-x-auto dark-scroll">
        <table className="w-full border-collapse font-mono text-[13px] leading-[1.6]">
          <tbody>
            {diff.hunks.map((line, idx) => {
              const isAdd = line.type === 'add';
              const isRemove = line.type === 'remove';
              return (
                <tr
                  key={`${idx}-${line.lineOld ?? 'x'}-${line.lineNew ?? 'y'}`}
                  className={
                    isAdd
                      ? 'bg-[#5db872]/14 text-[#d8f3dc]'
                      : isRemove
                      ? 'bg-[#c64545]/16 text-[#f9d6d6]'
                      : 'text-code-text/90'
                  }
                >
                  <td className="w-10 select-none text-right pr-2 pl-3 text-code-muted text-[11px] tabular-nums">
                    {line.lineOld ?? ''}
                  </td>
                  <td className="w-10 select-none text-right pr-3 text-code-muted text-[11px] tabular-nums border-r border-code-border">
                    {line.lineNew ?? ''}
                  </td>
                  <td className="w-6 select-none text-center font-medium">
                    {isAdd ? (
                      <span className="text-[#5db872]">+</span>
                    ) : isRemove ? (
                      <span className="text-[#e06666]">-</span>
                    ) : (
                      <span className="text-code-muted"> </span>
                    )}
                  </td>
                  <td className="pr-4 whitespace-pre">{line.content}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
