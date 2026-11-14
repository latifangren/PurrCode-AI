import React, { useEffect, useMemo, useState } from 'react';
import { Paperclip, Send, Square, X } from 'lucide-react';
import { matchSlashCommands } from '../lib/cockpitLogic';
import type { QuickCommandConfig } from '../types/cockpit';

interface PromptDockProps {
  disabled: boolean;
  isStreaming?: boolean;
  workspaceFiles?: string[];
  quickCommands?: QuickCommandConfig[];
  onSend: (prompt: string, pinnedFiles: string[]) => void;
  onStop?: () => void;
}

export function PromptDock({
  disabled,
  isStreaming = false,
  workspaceFiles = [],
  quickCommands = [],
  onSend,
  onStop,
}: PromptDockProps) {
  const [input, setInput] = useState('');
  const [pinnedFiles, setPinnedFiles] = useState<string[]>([]);
  const [showFilePicker, setShowFilePicker] = useState(false);
  const [customFileInput, setCustomFileInput] = useState('');

  useEffect(() => {
    if (!isStreaming || !onStop) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onStop();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [isStreaming, onStop]);

  const slashSuggestions = useMemo(() => {
    const base = matchSlashCommands(input);
    const trimmed = input.trim().toLowerCase();
    const customMatches = quickCommands
      .filter((cmd) => `/${cmd.name.toLowerCase()}`.startsWith(trimmed))
      .map((cmd) => ({
        command: `/${cmd.name}`,
        usage: `/${cmd.name}`,
        description: cmd.description || cmd.template,
        template: cmd.template,
      }));
    return [
      ...base.map((b) => ({ ...b, template: undefined as string | undefined })),
      ...customMatches,
    ];
  }, [input, quickCommands]);
  const showSlashMenu = input.startsWith('/') && !input.includes(' ') && slashSuggestions.length > 0;

  const handleSubmit = (e?: React.FormEvent) => {
    e?.preventDefault();
    if (disabled || !input.trim()) return;
    onSend(input.trim(), pinnedFiles);
    setInput('');
    setShowFilePicker(false);
  };

  const togglePin = (file: string) => {
    setPinnedFiles((prev) =>
      prev.includes(file) ? prev.filter((f) => f !== file) : [...prev, file]
    );
  };

  return (
    <div className="shrink-0 border-t border-hairline bg-canvas px-4 sm:px-6 py-3.5 transition-colors">
      <form onSubmit={handleSubmit} className="max-w-4xl mx-auto relative space-y-2.5">
        {/* Slash command autocomplete popover */}
        {showSlashMenu && (
          <div
            role="listbox"
            aria-label="Slash commands"
            className="absolute bottom-full left-0 right-0 mb-2 rounded-lg border border-hairline bg-canvas divide-y divide-hairline-soft z-20"
          >
            {slashSuggestions.map((item) => (
              <button
                key={item.command}
                type="button"
                onClick={() =>
                  setInput(
                    item.template
                      ? item.template
                      : item.command === '/clear'
                      ? '/clear'
                      : `${item.command} `
                  )
                }
                className="w-full px-3.5 py-2 flex items-center justify-between gap-3 text-left hover:bg-surface-card transition-colors cursor-pointer"
              >
                <span className="font-mono text-xs font-medium text-primary">
                  {item.usage}
                </span>
                <span className="text-xs text-muted truncate">{item.description}</span>
              </button>
            ))}
          </div>
        )}

        {/* File context picker popover */}
        {showFilePicker && (
          <div className="rounded-lg border border-hairline bg-surface-soft p-3 space-y-2">
            <div className="flex items-center justify-between">
              <span className="text-xs font-medium text-ink">Pin workspace file to prompt</span>
              <button
                type="button"
                onClick={() => setShowFilePicker(false)}
                aria-label="Close file picker"
                className="text-muted hover:text-ink cursor-pointer"
              >
                <X className="w-3.5 h-3.5" />
              </button>
            </div>
            <div className="flex flex-wrap gap-1.5">
              {workspaceFiles.length === 0 ? (
                <span className="text-xs text-muted">No indexed files returned by workspace.</span>
              ) : (
                workspaceFiles.map((file) => {
                  const selected = pinnedFiles.includes(file);
                  return (
                    <button
                      key={file}
                      type="button"
                      onClick={() => togglePin(file)}
                      className={`px-2.5 py-1 rounded-md font-mono text-xs border transition-colors cursor-pointer ${
                        selected
                          ? 'border-primary bg-primary text-on-primary'
                          : 'border-hairline bg-canvas text-body hover:bg-surface-card'
                      }`}
                    >
                      {file}
                    </button>
                  );
                })
              )}
            </div>
            <div className="flex items-center gap-2 pt-1">
              <input
                type="text"
                value={customFileInput}
                onChange={(e) => setCustomFileInput(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') {
                    e.preventDefault();
                    const trimmed = customFileInput.trim();
                    if (trimmed && !pinnedFiles.includes(trimmed)) {
                      setPinnedFiles((prev) => [...prev, trimmed]);
                    }
                    setCustomFileInput('');
                  }
                }}
                placeholder="Or enter a relative workspace path..."
                aria-label="Custom workspace file path"
                className="flex-1 h-7 rounded-md border border-hairline bg-canvas px-2.5 font-mono text-xs text-ink focus:outline-none focus:border-primary"
              />
              <button
                type="button"
                onClick={() => {
                  const trimmed = customFileInput.trim();
                  if (trimmed && !pinnedFiles.includes(trimmed)) {
                    setPinnedFiles((prev) => [...prev, trimmed]);
                  }
                  setCustomFileInput('');
                }}
                className="h-7 px-2.5 rounded-md border border-hairline bg-canvas hover:bg-surface-card text-xs text-ink cursor-pointer"
              >
                Add
              </button>
            </div>
          </div>
        )}

        {/* Pinned files strip */}
        {pinnedFiles.length > 0 && (
          <div className="flex flex-wrap items-center gap-2 text-xs">
            <span className="text-muted">Context:</span>
            {pinnedFiles.map((file) => (
              <span
                key={file}
                className="inline-flex items-center gap-1 rounded-md border border-hairline bg-surface-card px-2 py-0.5 font-mono text-[11px] text-ink"
              >
                <span>{file}</span>
                <button
                  type="button"
                  onClick={() => togglePin(file)}
                  aria-label={`Unpin ${file}`}
                  className="text-muted hover:text-error cursor-pointer"
                >
                  <X className="w-3 h-3" />
                </button>
              </span>
            ))}
          </div>
        )}

        {/* Prompt Input Surface */}
        <div className="rounded-lg border border-hairline bg-canvas focus-within:border-primary focus-within:ring-3 focus-within:ring-primary/15 transition-all p-3 space-y-2.5">
          <textarea
            rows={2}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault();
                handleSubmit();
              }
            }}
            placeholder="Ask Meowstro anything, pin context files, or type / for commands..."
            aria-label="Prompt input"
            className="w-full resize-none bg-transparent text-sm text-ink placeholder:text-muted-soft focus:outline-none leading-relaxed"
          />

          <div className="flex items-center justify-between gap-3 pt-1 border-t border-hairline-soft">
            <div className="flex items-center gap-1.5 flex-wrap">
              <button
                type="button"
                onClick={() => setShowFilePicker((p) => !p)}
                className="h-8 px-2.5 inline-flex items-center gap-1.5 rounded-md text-xs font-medium text-body hover:bg-surface-card transition-colors cursor-pointer"
              >
                <Paperclip className="w-3.5 h-3.5 text-muted" />
                <span>Pin File</span>
              </button>
              <button
                type="button"
                onClick={() => setInput('/plan ')}
                className="h-8 px-2.5 rounded-md font-mono text-xs text-muted hover:bg-surface-card hover:text-ink transition-colors cursor-pointer"
              >
                /plan
              </button>
              <button
                type="button"
                onClick={() => setInput('/preset ')}
                className="h-8 px-2.5 rounded-md font-mono text-xs text-muted hover:bg-surface-card hover:text-ink transition-colors cursor-pointer"
              >
                /preset
              </button>
              {quickCommands.map((cmd) => (
                <button
                  key={cmd.name}
                  type="button"
                  title={cmd.description || cmd.template}
                  onClick={() => setInput(cmd.template)}
                  className="h-8 px-2.5 rounded-md font-mono text-xs text-primary bg-surface-soft hover:bg-surface-card border border-hairline transition-colors cursor-pointer"
                >
                  /{cmd.name}
                </button>
              ))}
            </div>

            {isStreaming && onStop ? (
              <button
                type="button"
                onClick={onStop}
                aria-label="Stop generation"
                className="h-9 px-4 inline-flex items-center gap-1.5 rounded-md border border-error bg-canvas hover:bg-error hover:text-on-primary text-error text-xs font-medium transition-colors whitespace-nowrap shrink-0 cursor-pointer"
              >
                <Square className="w-3.5 h-3.5 fill-current" />
                <span>Stop</span>
                <kbd className="hidden sm:inline font-mono text-[10px] opacity-80">Esc</kbd>
              </button>
            ) : (
              <button
                type="submit"
                disabled={disabled || !input.trim()}
                className="h-9 px-4 inline-flex items-center gap-1.5 rounded-md bg-primary hover:bg-primary-active disabled:bg-primary-disabled disabled:text-muted text-on-primary text-xs font-medium transition-colors whitespace-nowrap shrink-0 cursor-pointer"
              >
                <Send className="w-3.5 h-3.5" />
                <span>{disabled ? 'Working...' : 'Send Prompt'}</span>
              </button>
            )}
          </div>
        </div>
      </form>
    </div>
  );
}
