import React, { useEffect, useMemo, useRef, useState } from 'react';
import { X } from 'lucide-react';
import {
  computeTaskProgress,
  pickCompanionReactionQuip,
} from '../lib/cockpitLogic';
import type {
  CompanionConfig,
  CompanionReaction,
  Message,
  Session,
  UnifiedDiff,
} from '../types/cockpit';
import { ApprovalCard } from './ApprovalCard';
import { CatCompanionSvg } from './CatCompanionSvg';
import { DiffViewer } from './DiffViewer';
import { PromptDock } from './PromptDock';
import { ProseContent } from './ProseContent';
import { TaskProgressBanner } from './TaskProgressBanner';
import { ToolChain } from './ToolChain';

interface ChatTimelineProps {
  session: Session;
  companion: CompanionConfig;
  companionReaction?: CompanionReaction;
  streamingMessage: Message | null;
  streamElapsedMs?: number;
  streamError: string | null;
  onSendPrompt: (prompt: string, pinnedFiles: string[]) => void;
  onStopStream?: () => void;
  onPetCompanion?: () => void;
  onResolveApproval: (approvalId: string, decision: 'approved' | 'denied' | 'expired') => void;
}

export function ChatTimeline({
  session,
  companion,
  companionReaction = null,
  streamingMessage,
  streamElapsedMs = 0,
  streamError,
  onSendPrompt,
  onStopStream,
  onPetCompanion,
  onResolveApproval,
}: ChatTimelineProps) {
  const [inspectingDiff, setInspectingDiff] = useState<UnifiedDiff | null>(null);
  const bottomRef = useRef<HTMLDivElement | null>(null);
  const prevCountRef = useRef<number>(session.messages.length);

  const allMessages = useMemo(
    () => (streamingMessage ? [...session.messages, streamingMessage] : session.messages),
    [session.messages, streamingMessage]
  );

  const taskProgress = useMemo(
    () => computeTaskProgress(allMessages, Boolean(streamingMessage)),
    [allMessages, streamingMessage]
  );

  const reactionQuip = pickCompanionReactionQuip(companionReaction, companion.name);

  useEffect(() => {
    if (allMessages.length > prevCountRef.current || Boolean(streamingMessage)) {
      bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
    }
    prevCountRef.current = allMessages.length;
  }, [allMessages.length, streamingMessage]);

  return (
    <div className="flex-1 min-h-0 flex flex-col">
      {taskProgress && (
        <TaskProgressBanner
          progress={taskProgress}
          elapsedMs={streamElapsedMs}
          celebrating={companionReaction === 'celebrate'}
        />
      )}

      {/* Scrollable Execution Timeline */}
      <div className="flex-1 overflow-y-auto px-4 sm:px-6 py-6">
        <div className="max-w-4xl mx-auto space-y-6">
          {/* Editorial Session Header */}
          <header className="pb-4 border-b border-hairline space-y-1.5">
            <h1 className="font-editorial text-3xl sm:text-4xl text-ink leading-tight">
              {session.title}
            </h1>
            <div className="flex flex-wrap items-center gap-2 text-xs text-muted">
              <span className="font-mono">{session.branch}</span>
              <span aria-hidden="true">·</span>
              <span>Preset: {session.preset}</span>
              <span aria-hidden="true">·</span>
              <span className="font-mono">{session.model}</span>
            </div>
          </header>

          {streamError && (
            <div
              role="alert"
              className="rounded-lg border border-error bg-canvas px-4 py-3 text-sm text-error"
            >
              {streamError}
            </div>
          )}

          {allMessages.length === 0 ? (
            <div className="rounded-xl border border-hairline bg-surface-card p-8 flex flex-col items-center text-center space-y-3">
              {companion.visibility !== 'hidden' && (
                <button
                  type="button"
                  onClick={onPetCompanion}
                  aria-label={`Pet companion ${companion.name}`}
                  className="rounded-xl p-1.5 hover:bg-surface-strong/70 transition-colors cursor-pointer"
                >
                  <CatCompanionSvg
                    companion={companion}
                    mood={session.preset === 'oyen' ? 'excited' : 'idle'}
                    reaction={companionReaction}
                    size="lg"
                  />
                </button>
              )}
              <h2 className="font-editorial text-2xl text-ink">
                {companion.name} is ready for instructions
              </h2>
              <p className="text-sm text-body max-w-md mx-auto leading-relaxed">
                {reactionQuip ?? (
                  <>
                    Ask {companion.name} to inspect code, propose a unified patch, or draft an architectural plan. Try typing{' '}
                    <code className="font-mono text-xs bg-canvas px-1.5 py-0.5 rounded border border-hairline">
                      /plan
                    </code>{' '}
                    below.
                  </>
                )}
              </p>
            </div>
          ) : (
            <div className="space-y-5">
              {allMessages.map((msg) => {
                if (msg.sender === 'user') {
                  return (
                    <article
                      key={msg.id}
                      className="rounded-lg bg-surface-card px-5 py-4 space-y-2"
                    >
                      <div className="flex items-center justify-between text-xs text-muted">
                        <span className="font-medium text-ink">Operator</span>
                        <span className="font-mono tabular-nums">{msg.timestamp}</span>
                      </div>
                      <ProseContent content={msg.content} />
                      {msg.pinnedFiles && msg.pinnedFiles.length > 0 && (
                        <div className="pt-1 flex flex-wrap gap-2 font-mono text-[11px] text-muted">
                          {msg.pinnedFiles.map((file) => (
                            <span key={file}>Pinned: {file}</span>
                          ))}
                        </div>
                      )}
                    </article>
                  );
                }

                return (
                  <article
                    key={msg.id}
                    className="rounded-lg border border-hairline bg-canvas px-5 py-4 space-y-3.5"
                  >
                    <div className="flex flex-wrap items-center justify-between gap-2 text-xs">
                      <div className="flex items-center gap-2">
                        <span className="font-medium text-ink">
                          {msg.agentRole ?? 'Meowstro'}
                        </span>
                        <span className="text-muted-soft" aria-hidden="true">
                          ·
                        </span>
                        <span className="text-muted">
                          {msg.agentTitle ?? 'Orchestrator'}
                        </span>
                        {msg.modelUsed && (
                          <>
                            <span className="text-muted-soft" aria-hidden="true">
                              ·
                            </span>
                            <span className="font-mono text-muted">{msg.modelUsed}</span>
                          </>
                        )}
                      </div>

                      <div className="flex items-center gap-2 font-mono text-[11px] text-muted tabular-nums">
                        {msg.isStreaming && (
                          <span className="inline-flex items-center gap-1 text-primary">
                            <span className="w-1.5 h-1.5 rounded-full bg-primary animate-paw-step" />
                            <span
                              className="w-1.5 h-1.5 rounded-full bg-primary animate-paw-step"
                              style={{ animationDelay: '150ms' }}
                            />
                            <span
                              className="w-1.5 h-1.5 rounded-full bg-primary animate-paw-step"
                              style={{ animationDelay: '300ms' }}
                            />
                          </span>
                        )}
                        {msg.thinkingDurationMs && <span>{msg.thinkingDurationMs}ms</span>}
                        <span>{msg.timestamp}</span>
                      </div>
                    </div>

                    {msg.toolSteps && msg.toolSteps.length > 0 && (
                      <ToolChain steps={msg.toolSteps} />
                    )}

                    <ProseContent
                      content={msg.content || (msg.isStreaming ? 'Synthesizing response...' : '')}
                    />

                    {msg.diff && (
                      <DiffViewer
                        diff={msg.diff}
                        onInspectFull={() => setInspectingDiff(msg.diff!)}
                      />
                    )}

                    {msg.approval && (
                      <ApprovalCard
                        approval={msg.approval}
                        onResolve={onResolveApproval}
                        onInspectDiff={msg.diff ? () => setInspectingDiff(msg.diff!) : undefined}
                      />
                    )}
                  </article>
                );
              })}
            </div>
          )}

          <div ref={bottomRef} />
        </div>
      </div>

      {/* Sticky Prompt Dock */}
      <PromptDock
        disabled={Boolean(streamingMessage)}
        isStreaming={Boolean(streamingMessage)}
        onSend={onSendPrompt}
        onStop={onStopStream}
      />

      {/* Full Diff Inspection Modal */}
      {inspectingDiff && (
        <div
          role="dialog"
          aria-modal="true"
          aria-label={`Full diff for ${inspectingDiff.filePath}`}
          className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-ink/50"
          onClick={() => setInspectingDiff(null)}
        >
          <div
            className="w-full max-w-4xl max-h-[85vh] rounded-xl border border-code-border bg-code-bg flex flex-col overflow-hidden"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="px-5 py-3.5 bg-code-elevated flex items-center justify-between">
              <span className="font-mono text-sm text-code-text">
                Full Unified Diff · {inspectingDiff.filePath}
              </span>
              <button
                type="button"
                onClick={() => setInspectingDiff(null)}
                aria-label="Close diff modal"
                className="h-8 w-8 inline-flex items-center justify-center rounded text-code-muted hover:text-code-text cursor-pointer"
              >
                <X className="w-4 h-4" />
              </button>
            </div>
            <div className="p-4 overflow-y-auto dark-scroll">
              <DiffViewer diff={inspectingDiff} />
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
