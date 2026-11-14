import React, { Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { ChatTimeline } from './components/ChatTimeline';
import { CockpitBootSkeleton, ModuleViewSkeleton } from './components/CockpitSkeletons';
import { CommandPalette } from './components/CommandPalette';
import { ErrorBoundary } from './components/ErrorBoundary';
import { InspectorHud } from './components/InspectorHud';
import { Sidebar } from './components/Sidebar';
import { ToastRegion } from './components/ToastRegion';
import { TopBar } from './components/TopBar';
import { useChatStream } from './hooks/useChatStream';
import { useCockpitStore } from './hooks/useCockpitStore';
import { useToastQueue } from './hooks/useToastQueue';
import {
  DEFAULT_PREFERENCES,
  applyThemeToDocument,
  deriveCompanionMood,
  resolveThemeTokens,
} from './themes/themeRegistry';
import type { CockpitView, CompanionReaction } from './types/cockpit';

const RoutingView = lazy(() =>
  import('./components/RoutingView').then((m) => ({ default: m.RoutingView }))
);
const ProvidersView = lazy(() =>
  import('./components/ProvidersView').then((m) => ({ default: m.ProvidersView }))
);
const SandboxView = lazy(() =>
  import('./components/SandboxView').then((m) => ({ default: m.SandboxView }))
);
const MemoryView = lazy(() =>
  import('./components/MemoryView').then((m) => ({ default: m.MemoryView }))
);
const McpView = lazy(() =>
  import('./components/McpView').then((m) => ({ default: m.McpView }))
);
const CronView = lazy(() =>
  import('./components/CronView').then((m) => ({ default: m.CronView }))
);
const AppearanceView = lazy(() =>
  import('./components/AppearanceView').then((m) => ({ default: m.AppearanceView }))
);
const AuditLogsView = lazy(() =>
  import('./components/AuditLogsView').then((m) => ({ default: m.AuditLogsView }))
);

export default function App() {
  const [activeView, setActiveView] = useState<CockpitView>('chat');
  const [sidebarOpen, setSidebarOpen] = useState<boolean>(() =>
    typeof window !== 'undefined' ? window.innerWidth >= 1024 : true
  );
  const [inspectorOpen, setInspectorOpen] = useState<boolean>(true);
  const [paletteOpen, setPaletteOpen] = useState<boolean>(false);
  const [companionReaction, setCompanionReaction] = useState<CompanionReaction>(null);
  const reactionTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const triggerReaction = useCallback((reaction: Exclude<CompanionReaction, null>) => {
    if (reactionTimerRef.current) {
      clearTimeout(reactionTimerRef.current);
    }
    setCompanionReaction(reaction);
    reactionTimerRef.current = setTimeout(() => {
      setCompanionReaction(null);
    }, 2600);
  }, []);

  const { toasts, pushToast, dismissToast } = useToastQueue();

  const store = useCockpitStore({
    pushToast,
    onCelebrate: () => triggerReaction('celebrate'),
  });

  const activeSession = useMemo(() => {
    if (!store.state) return null;
    return (
      store.state.sessions.find((s) => s.id === store.state!.activeSessionId) ??
      store.state.sessions[0] ??
      null
    );
  }, [store.state]);

  const {
    streamingMessage,
    streamError,
    streamElapsedMs,
    sendPrompt,
    stopStream,
  } = useChatStream({
    activeSessionId: activeSession?.id,
    setState: store.setState,
    pushToast,
    onCelebrate: () => triggerReaction('celebrate'),
  });

  const preferences = store.state?.preferences ?? DEFAULT_PREFERENCES;

  useEffect(() => {
    const tokens = resolveThemeTokens(
      preferences.activeThemeId,
      preferences.customThemeTokens
    );
    applyThemeToDocument(tokens);
  }, [preferences.activeThemeId, preferences.customThemeTokens]);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        setPaletteOpen((prev) => !prev);
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'b') {
        e.preventDefault();
        setSidebarOpen((prev) => !prev);
      } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'i') {
        e.preventDefault();
        setInspectorOpen((prev) => !prev);
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);

  const availableModels = useMemo(() => {
    if (!store.state) return ['antigravity/gemini-3.8-flash-tiered'];
    const set = new Set<string>(['antigravity/gemini-3.8-flash-tiered']);
    for (const p of store.state.providers) {
      for (const m of p.models) set.add(m);
    }
    return Array.from(set);
  }, [store.state]);

  const contextLimit = useMemo(() => {
    if (!store.state || !activeSession) return 128000;
    const provider = store.state.providers.find((p) =>
      p.models.includes(activeSession.model)
    );
    return provider?.contextWindow ?? 128000;
  }, [store.state, activeSession]);

  const hasPendingApproval = useMemo(() => {
    if (!activeSession) return false;
    return activeSession.messages.some((m) => m.approval?.status === 'pending');
  }, [activeSession]);

  const companionMood = useMemo(
    () =>
      deriveCompanionMood({
        isStreaming: Boolean(streamingMessage),
        hasPendingApproval,
        activePreset: activeSession?.preset ?? 'tabby',
      }),
    [streamingMessage, hasPendingApproval, activeSession?.preset]
  );

  const handlePetCompanion = useCallback(() => {
    triggerReaction('petted');
  }, [triggerReaction]);

  if (store.loadError) {
    return (
      <div className="min-h-screen bg-canvas flex items-center justify-center p-6">
        <div className="max-w-md rounded-xl border border-hairline bg-surface-card p-6 space-y-3">
          <h1 className="font-editorial text-2xl text-ink">Cockpit Unavailable</h1>
          <p className="text-sm text-body">{store.loadError}</p>
          <button
            type="button"
            onClick={store.retryLoad}
            className="h-9 px-4 rounded-md bg-primary text-on-primary text-xs font-medium cursor-pointer"
          >
            Retry Connection
          </button>
        </div>
      </div>
    );
  }

  if (!store.state || !activeSession) {
    return <CockpitBootSkeleton />;
  }

  const handleNewSession = async () => {
    const created = await store.createSession();
    if (created) setActiveView('chat');
  };

  return (
    <div className="h-screen w-screen overflow-hidden flex flex-col bg-canvas text-ink">
      <TopBar
        projectName={store.state.projectName}
        activeSession={activeSession}
        availableModels={availableModels}
        isStreaming={Boolean(streamingMessage)}
        sidebarOpen={sidebarOpen}
        inspectorOpen={inspectorOpen}
        companion={preferences.companion}
        companionMood={companionMood}
        onToggleSidebar={() => setSidebarOpen((p) => !p)}
        onToggleInspector={() => setInspectorOpen((p) => !p)}
        onChangePreset={(preset) =>
          store.updateSessionMeta(activeSession.id, { preset })
        }
        onChangeModel={(model) =>
          store.updateSessionMeta(activeSession.id, { model })
        }
        onOpenPalette={() => setPaletteOpen(true)}
        onOpenAppearance={() => setActiveView('appearance')}
        onNewSession={handleNewSession}
      />

      <div className="flex-1 min-h-0 flex overflow-hidden">
        <Sidebar
          open={sidebarOpen}
          activeView={activeView}
          sessions={store.state.sessions}
          activeSessionId={activeSession.id}
          routing={store.state.routing}
          onCloseMobile={() => {
            if (typeof window !== 'undefined' && window.innerWidth < 1024) {
              setSidebarOpen(false);
            }
          }}
          onSelectView={setActiveView}
          onSelectSession={store.selectSession}
          onDeleteSession={store.deleteSession}
        />

        <main className="flex-1 min-w-0 min-h-0 flex flex-col bg-canvas transition-colors">
          <ErrorBoundary resetKey={activeView}>
            {activeView === 'chat' && (
              <ChatTimeline
                session={activeSession}
                companion={preferences.companion}
                companionReaction={companionReaction}
                streamingMessage={streamingMessage}
                streamElapsedMs={streamElapsedMs}
                streamError={streamError}
                onSendPrompt={sendPrompt}
                onStopStream={stopStream}
                onPetCompanion={handlePetCompanion}
                onResolveApproval={store.resolveApproval}
              />
            )}

            <Suspense fallback={<ModuleViewSkeleton />}>
              {activeView === 'routing' && (
                <RoutingView
                  routing={store.state.routing}
                  availableModels={availableModels}
                  onSaveRouting={store.saveRouting}
                />
              )}

              {activeView === 'providers' && (
                <ProvidersView
                  providers={store.state.providers}
                  onAddProvider={store.addProvider}
                  onToggleProvider={store.toggleProvider}
                  onDeleteProvider={store.deleteProvider}
                  onTestLatency={store.testLatency}
                />
              )}

              {activeView === 'sandbox' && (
                <SandboxView
                  sandbox={store.state.sandbox}
                  onAddPath={store.addSandboxPath}
                  onRemovePath={store.removeSandboxPath}
                  onToggleOutbound={store.toggleOutbound}
                />
              )}

              {activeView === 'memory' && (
                <MemoryView
                  memoryFiles={store.state.memoryFiles}
                  skills={store.state.skills}
                  onSaveMemory={store.saveMemory}
                  onManageSkill={store.manageSkill}
                />
              )}

              {activeView === 'mcp' && (
                <McpView
                  servers={store.state.mcpServers}
                  onAddServer={store.addMcpServer}
                  onPingServer={store.pingMcpServer}
                  onDeleteServer={store.deleteMcpServer}
                />
              )}

              {activeView === 'cron' && (
                <CronView
                  jobs={store.state.cronJobs}
                  onAddJob={store.addCronJob}
                  onToggleJob={store.toggleCronJob}
                  onDeleteJob={store.deleteCronJob}
                />
              )}

              {activeView === 'appearance' && (
                <AppearanceView
                  preferences={preferences}
                  onUpdatePreferences={store.updatePreferences}
                />
              )}

              {activeView === 'logs' && (
                <AuditLogsView initialAuditLog={store.state.sandbox.auditLog} />
              )}
            </Suspense>
          </ErrorBoundary>
        </main>

        <InspectorHud
          open={inspectorOpen}
          session={activeSession}
          contextLimit={contextLimit}
          routing={store.state.routing}
          sandbox={store.state.sandbox}
          companion={preferences.companion}
          companionMood={companionMood}
          companionReaction={companionReaction}
          onPetCompanion={handlePetCompanion}
          onClose={() => setInspectorOpen(false)}
          onOpenSandbox={() => setActiveView('sandbox')}
          onOpenAppearance={() => setActiveView('appearance')}
        />
      </div>

      <CommandPalette
        open={paletteOpen}
        sessions={store.state.sessions}
        availableModels={availableModels}
        onClose={() => setPaletteOpen(false)}
        onSelectView={setActiveView}
        onSelectSession={store.selectSession}
        onChangePreset={(preset) =>
          store.updateSessionMeta(activeSession.id, { preset })
        }
        onChangeModel={(model) =>
          store.updateSessionMeta(activeSession.id, { model })
        }
        onSelectTheme={(activeThemeId) =>
          store.updatePreferences({ ...preferences, activeThemeId })
        }
        onNewSession={handleNewSession}
      />

      <ToastRegion toasts={toasts} onDismiss={dismissToast} />
    </div>
  );
}
