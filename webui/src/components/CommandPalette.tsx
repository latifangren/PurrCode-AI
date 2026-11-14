import React, { useEffect, useState } from 'react';
import { Search, X } from 'lucide-react';
import { THEME_PRESETS } from '../themes/themeRegistry';
import type { CockpitView, PresetMode, Session } from '../types/cockpit';

interface CommandPaletteProps {
  open: boolean;
  sessions: Session[];
  availableModels: string[];
  onClose: () => void;
  onSelectView: (view: CockpitView) => void;
  onSelectSession: (id: string) => void;
  onChangePreset: (preset: PresetMode) => void;
  onChangeModel: (model: string) => void;
  onSelectTheme: (themeId: string) => void;
  onNewSession: () => void;
}

interface CommandEntry {
  id: string;
  group: string;
  label: string;
  detail: string;
  action: () => void;
}

export function CommandPalette({
  open,
  sessions,
  availableModels,
  onClose,
  onSelectView,
  onSelectSession,
  onChangePreset,
  onChangeModel,
  onSelectTheme,
  onNewSession,
}: CommandPaletteProps) {
  const [query, setQuery] = useState('');

  useEffect(() => {
    if (open) setQuery('');
  }, [open]);

  if (!open) return null;

  const commands: CommandEntry[] = [
    {
      id: 'cmd-new-session',
      group: 'Sessions',
      label: 'Create new session',
      detail: 'Start a fresh timeline with Meowstro',
      action: () => {
        onNewSession();
        onClose();
      },
    },
    {
      id: 'cmd-view-chat',
      group: 'Navigation',
      label: 'Go to Execution Timeline',
      detail: 'Chat & active approvals',
      action: () => {
        onSelectView('chat');
        onClose();
      },
    },
    {
      id: 'cmd-view-routing',
      group: 'Navigation',
      label: 'Go to Clowder Matrix',
      detail: 'Subagent model mappings',
      action: () => {
        onSelectView('routing');
        onClose();
      },
    },
    {
      id: 'cmd-view-providers',
      group: 'Navigation',
      label: 'Go to Providers & Models',
      detail: 'Endpoints & latency test',
      action: () => {
        onSelectView('providers');
        onClose();
      },
    },
    {
      id: 'cmd-view-sandbox',
      group: 'Navigation',
      label: 'Go to Landlock Sandbox',
      detail: 'Kernel filesystem paths & SSRF shield',
      action: () => {
        onSelectView('sandbox');
        onClose();
      },
    },
    {
      id: 'cmd-view-memory',
      group: 'Navigation',
      label: 'Go to Memory & Skills',
      detail: 'MEMORY.md, USER.md, SOUL.md & skills',
      action: () => {
        onSelectView('memory');
        onClose();
      },
    },
    {
      id: 'cmd-view-mcp',
      group: 'Navigation',
      label: 'Go to MCP Server Hub',
      detail: 'Stdio/SSE tool schemas',
      action: () => {
        onSelectView('mcp');
        onClose();
      },
    },
    {
      id: 'cmd-view-cron',
      group: 'Navigation',
      label: 'Go to Cron Scheduler',
      detail: 'Natural language automation',
      action: () => {
        onSelectView('cron');
        onClose();
      },
    },
    {
      id: 'cmd-view-appearance',
      group: 'Navigation',
      label: 'Go to Themes & Companion',
      detail: 'Modular color themes & feline companion studio',
      action: () => {
        onSelectView('appearance');
        onClose();
      },
    },
    {
      id: 'cmd-view-logs',
      group: 'Navigation',
      label: 'Go to Audit Logs',
      detail: 'Real-time security logs, tool audits, and execution trail',
      action: () => {
        onSelectView('logs');
        onClose();
      },
    },
    ...THEME_PRESETS.map((t) => ({
      id: `cmd-theme-${t.id}`,
      group: 'Themes',
      label: `Switch theme to ${t.name}`,
      detail: t.description,
      action: () => {
        onSelectTheme(t.id);
        onClose();
      },
    })),
    ...(['tabby', 'carrier', 'oyen'] as PresetMode[]).map((p) => ({
      id: `cmd-preset-${p}`,
      group: 'Presets',
      label: `Switch preset to ${p}`,
      detail: p === 'tabby' ? 'Supervised approval gate' : p === 'carrier' ? 'Balanced mode' : 'Autonomous mode',
      action: () => {
        onChangePreset(p);
        onClose();
      },
    })),
    ...availableModels.map((m) => ({
      id: `cmd-model-${m}`,
      group: 'Models',
      label: `Switch active model to ${m}`,
      detail: 'Update current session model',
      action: () => {
        onChangeModel(m);
        onClose();
      },
    })),
    ...sessions.map((s) => ({
      id: `cmd-session-${s.id}`,
      group: 'Sessions',
      label: `Open session: ${s.title}`,
      detail: `${s.branch} · ${s.updatedAt}`,
      action: () => {
        onSelectSession(s.id);
        onSelectView('chat');
        onClose();
      },
    })),
  ];

  const q = query.trim().toLowerCase();
  const filtered = q
    ? commands.filter(
        (c) =>
          c.label.toLowerCase().includes(q) ||
          c.detail.toLowerCase().includes(q) ||
          c.group.toLowerCase().includes(q)
      )
    : commands;

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Command palette"
      className="fixed inset-0 z-50 flex items-start justify-center pt-[12vh] px-4 bg-ink/40"
      onClick={onClose}
    >
      <div
        className="w-full max-w-xl rounded-xl border border-hairline bg-canvas overflow-hidden"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-2.5 px-4 py-3 border-b border-hairline">
          <Search className="w-4 h-4 text-muted shrink-0" />
          <input
            type="text"
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') onClose();
              if (e.key === 'Enter' && filtered.length > 0) {
                filtered[0].action();
              }
            }}
            placeholder="Jump to view, switch theme, change model, or open session..."
            className="flex-1 bg-transparent text-sm text-ink placeholder:text-muted-soft focus:outline-none"
          />
          <button
            type="button"
            onClick={onClose}
            aria-label="Close command palette"
            className="h-7 w-7 inline-flex items-center justify-center rounded text-muted hover:text-ink cursor-pointer"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        <div className="max-h-[360px] overflow-y-auto divide-y divide-hairline-soft">
          {filtered.length === 0 ? (
            <div className="px-4 py-8 text-center text-sm text-muted">
              No matching commands found.
            </div>
          ) : (
            filtered.map((cmd) => (
              <button
                key={cmd.id}
                type="button"
                onClick={cmd.action}
                className="w-full flex items-center justify-between gap-4 px-4 py-2.5 text-left hover:bg-surface-card transition-colors cursor-pointer"
              >
                <div className="min-w-0">
                  <div className="text-sm font-medium text-ink truncate">{cmd.label}</div>
                  <div className="text-xs text-muted truncate">{cmd.detail}</div>
                </div>
                <span className="text-[11px] font-mono text-muted-soft shrink-0">
                  {cmd.group}
                </span>
              </button>
            ))
          )}
        </div>
      </div>
    </div>
  );
}
