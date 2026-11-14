import React from 'react';
import {
  BookOpen,
  Clock,
  Cpu,
  GitBranch,
  MessageSquare,
  Palette,
  Plug,
  Shield,
  Trash2,
  X,
} from 'lucide-react';
import type { CockpitView, Session, SubagentRouting } from '../types/cockpit';

interface SidebarProps {
  open: boolean;
  activeView: CockpitView;
  sessions: Session[];
  activeSessionId: string;
  routing: SubagentRouting[];
  onCloseMobile: () => void;
  onSelectView: (view: CockpitView) => void;
  onSelectSession: (id: string) => void;
  onDeleteSession: (id: string) => void;
}

const NAV_ITEMS: { id: CockpitView; label: string; icon: React.ComponentType<{ className?: string }> }[] = [
  { id: 'chat', label: 'Execution Timeline', icon: MessageSquare },
  { id: 'routing', label: 'Clowder Matrix', icon: GitBranch },
  { id: 'providers', label: 'Providers & Models', icon: Cpu },
  { id: 'sandbox', label: 'Landlock Sandbox', icon: Shield },
  { id: 'memory', label: 'Memory & Skills', icon: BookOpen },
  { id: 'mcp', label: 'MCP Server Hub', icon: Plug },
  { id: 'cron', label: 'Cron Scheduler', icon: Clock },
  { id: 'appearance', label: 'Themes & Companion', icon: Palette },
];

export function Sidebar({
  open,
  activeView,
  sessions,
  activeSessionId,
  routing,
  onCloseMobile,
  onSelectView,
  onSelectSession,
  onDeleteSession,
}: SidebarProps) {
  if (!open) return null;

  return (
    <>
      {/* Mobile backdrop */}
      <div
        className="fixed inset-0 z-30 bg-ink/30 lg:hidden"
        onClick={onCloseMobile}
        aria-hidden="true"
      />

      <aside
        aria-label="Workspace navigation"
        className="fixed inset-y-0 left-0 z-40 w-[264px] shrink-0 border-r border-hairline bg-surface-soft flex flex-col lg:static lg:z-auto transition-colors"
      >
        <div className="flex items-center justify-between px-4 py-3 border-b border-hairline lg:hidden">
          <span className="font-serif text-lg text-ink">Workspace</span>
          <button
            type="button"
            onClick={onCloseMobile}
            aria-label="Close sidebar"
            className="h-8 w-8 inline-flex items-center justify-center rounded-md text-muted hover:text-ink"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        <div className="flex-1 overflow-y-auto px-3 py-4 space-y-6">
          {/* Primary Cockpit Navigation */}
          <nav aria-label="Cockpit modules" className="space-y-1">
            <p className="px-2.5 pb-1.5 text-[11px] font-medium tracking-[0.08em] uppercase text-muted">
              Control Plane
            </p>
            {NAV_ITEMS.map((item) => {
              const Icon = item.icon;
              const isActive = activeView === item.id;
              return (
                <button
                  key={item.id}
                  type="button"
                  onClick={() => {
                    onSelectView(item.id);
                    onCloseMobile();
                  }}
                  className={`w-full flex items-center gap-2.5 px-2.5 py-2 rounded-md text-sm font-medium transition-colors cursor-pointer whitespace-nowrap ${
                    isActive
                      ? 'bg-surface-strong text-ink'
                      : 'text-body hover:bg-surface-card hover:text-ink'
                  }`}
                >
                  <Icon className={`w-4 h-4 shrink-0 ${isActive ? 'text-primary' : 'text-muted'}`} />
                  <span className="truncate">{item.label}</span>
                </button>
              );
            })}
          </nav>

          {/* Sessions List */}
          <div className="space-y-1.5 pt-2 border-t border-hairline">
            <p className="px-2.5 pb-1 text-[11px] font-medium tracking-[0.08em] uppercase text-muted">
              Sessions ({sessions.length})
            </p>
            {sessions.map((session) => {
              const isCurrent = session.id === activeSessionId && activeView === 'chat';
              return (
                <div
                  key={session.id}
                  className={`group flex items-center justify-between gap-1 rounded-md px-2.5 py-2 transition-colors ${
                    isCurrent ? 'bg-surface-card' : 'hover:bg-surface-card/60'
                  }`}
                >
                  <button
                    type="button"
                    onClick={() => {
                      onSelectSession(session.id);
                      onSelectView('chat');
                      onCloseMobile();
                    }}
                    className="flex-1 min-w-0 text-left cursor-pointer"
                  >
                    <div className="text-[13px] font-medium text-ink truncate">
                      {session.title}
                    </div>
                    <div className="text-[11px] text-muted truncate tabular-nums">
                      {session.preset} · {session.updatedAt}
                    </div>
                  </button>

                  {sessions.length > 1 && (
                    <button
                      type="button"
                      onClick={() => onDeleteSession(session.id)}
                      aria-label={`Delete session ${session.title}`}
                      className="opacity-0 group-hover:opacity-100 focus:opacity-100 h-7 w-7 shrink-0 inline-flex items-center justify-center rounded text-muted hover:text-error transition-opacity cursor-pointer"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </button>
                  )}
                </div>
              );
            })}
          </div>

          {/* Clowder Team Roster */}
          <div className="space-y-1.5 pt-2 border-t border-hairline">
            <p className="px-2.5 pb-1 text-[11px] font-medium tracking-[0.08em] uppercase text-muted">
              Clowder Team
            </p>
            <div className="space-y-1 px-2.5">
              {routing.map((agent) => (
                <div
                  key={agent.role}
                  className="flex items-center justify-between py-1 text-xs"
                >
                  <span className="font-medium text-body-strong">{agent.role}</span>
                  <span
                    className={`font-mono text-[11px] ${
                      agent.status === 'active' ? 'text-primary font-medium' : 'text-muted-soft'
                    }`}
                  >
                    {agent.status === 'active' ? 'Active' : 'Idle'}
                  </span>
                </div>
              ))}
            </div>
          </div>
        </div>
      </aside>
    </>
  );
}
