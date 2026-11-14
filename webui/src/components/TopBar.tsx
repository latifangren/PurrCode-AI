import React from 'react';
import {
  Command,
  PanelLeft,
  PanelRight,
  Plus,
} from 'lucide-react';
import type { CompanionConfig, CompanionMood, PresetMode, Session } from '../types/cockpit';
import { BrandMark } from './BrandMark';
import { CatCompanionSvg } from './CatCompanionSvg';

interface TopBarProps {
  projectName: string;
  activeSession: Session;
  availableModels: string[];
  isStreaming: boolean;
  sidebarOpen: boolean;
  inspectorOpen: boolean;
  companion: CompanionConfig;
  companionMood: CompanionMood;
  onToggleSidebar: () => void;
  onToggleInspector: () => void;
  onChangePreset: (preset: PresetMode) => void;
  onChangeModel: (model: string) => void;
  onOpenPalette: () => void;
  onOpenAppearance: () => void;
  onNewSession: () => void;
}

export function TopBar({
  projectName,
  activeSession,
  availableModels,
  isStreaming,
  sidebarOpen,
  inspectorOpen,
  companion,
  companionMood,
  onToggleSidebar,
  onToggleInspector,
  onChangePreset,
  onChangeModel,
  onOpenPalette,
  onOpenAppearance,
  onNewSession,
}: TopBarProps) {
  return (
    <header className="h-14 shrink-0 border-b border-hairline bg-canvas px-3 sm:px-5 flex items-center justify-between gap-3 transition-colors">
      {/* Zone 1: Brand & Breadcrumb */}
      <div className="flex items-center gap-2.5 min-w-0">
        <button
          type="button"
          onClick={onToggleSidebar}
          aria-label={sidebarOpen ? 'Collapse navigation sidebar' : 'Expand navigation sidebar'}
          className="h-9 w-9 shrink-0 inline-flex items-center justify-center rounded-md border border-hairline bg-canvas text-ink hover:bg-surface-card transition-colors cursor-pointer"
        >
          <PanelLeft className="w-4 h-4" />
        </button>

        <div className="flex items-center gap-2 min-w-0">
          <BrandMark className="w-6 h-6 shrink-0" />
          <span className="font-serif text-xl tracking-[-0.02em] text-ink whitespace-nowrap shrink-0">
            PurrCode
          </span>
          <span className="hidden md:inline text-muted-soft select-none" aria-hidden="true">
            /
          </span>
          <span className="hidden md:inline text-sm text-muted truncate max-w-[140px]">
            {projectName}
          </span>
          <span className="hidden lg:inline text-muted-soft select-none" aria-hidden="true">
            ·
          </span>
          <span className="hidden lg:inline font-mono text-xs text-muted whitespace-nowrap">
            {activeSession.branch}
          </span>
        </div>
      </div>

      {/* Zone 2: Active Controls */}
      <div className="hidden md:flex items-center gap-3 shrink-0">
        <label className="flex items-center gap-1.5 text-xs text-muted whitespace-nowrap">
          <span>Preset</span>
          <select
            aria-label="Execution preset"
            value={activeSession.preset}
            onChange={(e) => onChangePreset(e.target.value as PresetMode)}
            className="h-8 rounded-md border border-hairline bg-canvas px-2.5 text-xs font-medium text-ink focus:outline-none focus:border-primary"
          >
            <option value="tabby">tabby (supervised)</option>
            <option value="carrier">carrier (balanced)</option>
            <option value="oyen">oyen (autonomous)</option>
          </select>
        </label>

        <label className="flex items-center gap-1.5 text-xs text-muted whitespace-nowrap">
          <span>Model</span>
          <select
            aria-label="Active session model"
            value={activeSession.model}
            onChange={(e) => onChangeModel(e.target.value)}
            className="h-8 rounded-md border border-hairline bg-canvas px-2.5 font-mono text-xs text-ink focus:outline-none focus:border-primary max-w-[180px]"
          >
            {availableModels.map((model) => (
              <option key={model} value={model}>
                {model}
              </option>
            ))}
          </select>
        </label>

        {isStreaming && (
          <span className="inline-flex items-center gap-1.5 text-xs font-medium text-primary whitespace-nowrap pl-1">
            <span className="w-1.5 h-1.5 rounded-full bg-primary animate-paw-step" />
            <span>Thinking</span>
          </span>
        )}
      </div>

      {/* Zone 3: Primary Actions */}
      <div className="flex items-center gap-2 shrink-0">
        {companion.visibility === 'compact' && (
          <button
            type="button"
            onClick={onOpenAppearance}
            aria-label={`Customize companion ${companion.name}`}
            className="hidden sm:inline-flex h-9 px-2 items-center gap-1.5 rounded-md border border-hairline bg-surface-soft hover:bg-surface-card transition-colors cursor-pointer"
          >
            <CatCompanionSvg companion={companion} mood={companionMood} size="sm" />
            <span className="hidden lg:inline text-xs font-medium text-ink">{companion.name}</span>
          </button>
        )}

        <button
          type="button"
          onClick={onOpenPalette}
          aria-label="Open command palette"
          className="h-9 px-3 inline-flex items-center gap-1.5 rounded-md border border-hairline bg-canvas text-xs font-medium text-ink hover:bg-surface-card transition-colors whitespace-nowrap shrink-0 cursor-pointer"
        >
          <Command className="w-3.5 h-3.5 text-muted" />
          <span className="hidden sm:inline">Command</span>
          <kbd className="hidden lg:inline font-mono text-[11px] text-muted">⌘K</kbd>
        </button>

        <button
          type="button"
          onClick={onNewSession}
          className="h-9 px-3.5 inline-flex items-center gap-1.5 rounded-md bg-primary hover:bg-primary-active text-on-primary text-xs font-medium transition-colors whitespace-nowrap shrink-0 cursor-pointer"
        >
          <Plus className="w-3.5 h-3.5" />
          <span>New Session</span>
        </button>

        <button
          type="button"
          onClick={onToggleInspector}
          aria-label={inspectorOpen ? 'Hide inspector HUD' : 'Show inspector HUD'}
          className="hidden xl:inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-md border border-hairline bg-canvas text-ink hover:bg-surface-card transition-colors cursor-pointer"
        >
          <PanelRight className="w-4 h-4" />
        </button>
      </div>
    </header>
  );
}
