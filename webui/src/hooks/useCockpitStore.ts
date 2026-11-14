import { useCallback, useEffect, useState } from 'react';
import { requestJson } from '../lib/apiClient';
import type {
  AgentRole,
  AuditEntry,
  CockpitPreferences,
  CockpitState,
  CronJob,
  McpServer,
  MemoryFile,
  PresetMode,
  ProviderConfig,
  SandboxState,
  Session,
  SkillEntry,
  SubagentRouting,
  ToastTone,
} from '../types/cockpit';

interface UseCockpitStoreOptions {
  pushToast: (tone: ToastTone, title: string, detail?: string) => void;
  onCelebrate: () => void;
}

export function useCockpitStore({
  pushToast,
  onCelebrate,
}: UseCockpitStoreOptions) {
  const [state, setState] = useState<CockpitState | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const fetchState = useCallback(async () => {
    setLoadError(null);
    try {
      const data = await requestJson<CockpitState>('/api/state');
      setState(data);
    } catch (err) {
      setLoadError(
        err instanceof Error ? err.message : 'Failed to load cockpit state.'
      );
    }
  }, []);

  useEffect(() => {
    fetchState();
  }, [fetchState]);

  const runMutation = useCallback(
    async <T,>(
      fn: () => Promise<T>,
      errorTitle: string
    ): Promise<T | null> => {
      try {
        return await fn();
      } catch (err) {
        const detail = err instanceof Error ? err.message : 'Unexpected server error';
        pushToast('error', errorTitle, detail);
        return null;
      }
    },
    [pushToast]
  );

  const updatePreferences = useCallback(
    async (next: CockpitPreferences) => {
      setState((prev) => (prev ? { ...prev, preferences: next } : prev));
      const res = await runMutation(
        () =>
          requestJson<{ preferences: CockpitPreferences }>('/api/preferences', {
            method: 'PUT',
            body: JSON.stringify({ preferences: next }),
          }),
        'Failed to save appearance preferences'
      );
      if (res) {
        pushToast('success', 'Appearance updated', `${next.companion.name} & theme synced.`);
      }
    },
    [pushToast, runMutation]
  );

  const createSession = useCallback(async () => {
    const data = await runMutation(
      () =>
        requestJson<{ session: Session; activeSessionId: string }>(
          '/api/sessions',
          { method: 'POST', body: JSON.stringify({}) }
        ),
      'Failed to create session'
    );
    if (data) {
      setState((prev) =>
        prev
          ? {
              ...prev,
              sessions: [data.session, ...prev.sessions],
              activeSessionId: data.activeSessionId,
            }
          : prev
      );
      pushToast('info', 'New session created', data.session.title);
      return true;
    }
    return false;
  }, [pushToast, runMutation]);

  const selectSession = useCallback(
    async (id: string) => {
      setState((prev) => (prev ? { ...prev, activeSessionId: id } : prev));
      await runMutation(
        () =>
          requestJson(`/api/sessions/${id}`, {
            method: 'PATCH',
            body: JSON.stringify({ activate: true }),
          }),
        'Failed to switch session'
      );
    },
    [runMutation]
  );

  const deleteSession = useCallback(
    async (id: string) => {
      const data = await runMutation(
        () =>
          requestJson<{ sessions: Session[]; activeSessionId: string }>(
            `/api/sessions/${id}`,
            { method: 'DELETE' }
          ),
        'Could not delete session'
      );
      if (data) {
        setState((prev) =>
          prev
            ? {
                ...prev,
                sessions: data.sessions,
                activeSessionId: data.activeSessionId,
              }
            : prev
        );
        pushToast('info', 'Session removed');
      }
    },
    [pushToast, runMutation]
  );

  const updateSessionMeta = useCallback(
    async (sessionId: string, patch: { preset?: PresetMode; model?: string }) => {
      setState((prev) =>
        prev
          ? {
              ...prev,
              sessions: prev.sessions.map((s) =>
                s.id === sessionId ? { ...s, ...patch } : s
              ),
            }
          : prev
      );
      await runMutation(
        () =>
          requestJson(`/api/sessions/${sessionId}`, {
            method: 'PATCH',
            body: JSON.stringify(patch),
          }),
        'Failed to update session settings'
      );
    },
    [runMutation]
  );

  const resolveApproval = useCallback(
    async (
      approvalId: string,
      decision: 'approved' | 'denied' | 'expired'
    ) => {
      const data = await runMutation(
        () =>
          requestJson<{ sessions: Session[]; sandbox: SandboxState }>(
            `/api/approvals/${approvalId}`,
            {
              method: 'POST',
              body: JSON.stringify({ decision }),
            }
          ),
        'Failed to submit approval decision'
      );
      if (data) {
        setState((prev) =>
          prev
            ? {
                ...prev,
                sessions: data.sessions,
                sandbox: data.sandbox,
              }
            : prev
        );
        if (decision === 'approved') {
          onCelebrate();
          pushToast('success', 'Patch approved', 'Execution finalized cleanly.');
        } else {
          pushToast('warning', decision === 'expired' ? 'Approval expired' : 'Patch denied', 'Fail-closed guard blocked apply.');
        }
      }
    },
    [onCelebrate, pushToast, runMutation]
  );

  const saveRouting = useCallback(
    async (nextRouting: SubagentRouting[]) => {
      const data = await runMutation(
        () =>
          requestJson<{ routing: SubagentRouting[]; auditLog: AuditEntry[] }>(
            '/api/routing',
            {
              method: 'PUT',
              body: JSON.stringify({ routing: nextRouting }),
            }
          ),
        'Failed to save Clowder routing'
      );
      if (data) {
        setState((prev) =>
          prev
            ? {
                ...prev,
                routing: data.routing,
                sandbox: { ...prev.sandbox, auditLog: data.auditLog },
              }
            : prev
        );
        pushToast('success', 'Routing matrix saved');
      }
    },
    [pushToast, runMutation]
  );

  const addProvider = useCallback(
    async (payload: {
      name: string;
      protocol: ProviderConfig['protocol'];
      baseUrl: string;
      defaultModel: string;
      contextWindow: number;
    }) => {
      const data = await runMutation(
        () =>
          requestJson<{ providers: ProviderConfig[]; auditLog: AuditEntry[] }>(
            '/api/providers',
            {
              method: 'POST',
              body: JSON.stringify(payload),
            }
          ),
        'Failed to add provider'
      );
      if (data) {
        setState((prev) =>
          prev
            ? {
                ...prev,
                providers: data.providers,
                sandbox: { ...prev.sandbox, auditLog: data.auditLog },
              }
            : prev
        );
        pushToast('success', 'Provider added', payload.name);
      }
    },
    [pushToast, runMutation]
  );

  const toggleProvider = useCallback(
    async (id: string, enabled: boolean) => {
      const data = await runMutation(
        () =>
          requestJson<{ providers: ProviderConfig[] }>(`/api/providers/${id}`, {
            method: 'PATCH',
            body: JSON.stringify({ enabled }),
          }),
        'Failed to toggle provider'
      );
      if (data) {
        setState((prev) => (prev ? { ...prev, providers: data.providers } : prev));
      }
    },
    [runMutation]
  );

  const deleteProvider = useCallback(
    async (id: string) => {
      const data = await runMutation(
        () =>
          requestJson<{ providers: ProviderConfig[] }>(`/api/providers/${id}`, {
            method: 'DELETE',
          }),
        'Failed to delete provider'
      );
      if (data) {
        setState((prev) => (prev ? { ...prev, providers: data.providers } : prev));
        pushToast('info', 'Provider removed');
      }
    },
    [pushToast, runMutation]
  );

  const testLatency = useCallback(
    async (id: string) => {
      const data = await runMutation(
        () =>
          requestJson<{ provider: ProviderConfig; providers: ProviderConfig[] }>(
            `/api/providers/${id}/test`,
            { method: 'POST' }
          ),
        'Latency probe failed'
      );
      if (data) {
        setState((prev) => (prev ? { ...prev, providers: data.providers } : prev));
        pushToast(
          'info',
          `${data.provider.name} pinged`,
          `${data.provider.lastLatencyMs ?? '—'}ms round-trip`
        );
      }
    },
    [pushToast, runMutation]
  );

  const addSandboxPath = useCallback(
    async (pathStr: string, mode: 'ro' | 'rw'): Promise<string | null> => {
      try {
        const data = await requestJson<{ sandbox: SandboxState }>(
          '/api/sandbox/paths',
          {
            method: 'POST',
            body: JSON.stringify({ path: pathStr, mode }),
          }
        );
        setState((prev) => (prev ? { ...prev, sandbox: data.sandbox } : prev));
        pushToast('success', 'Sandbox rule added', `${mode.toUpperCase()} ${pathStr}`);
        return null;
      } catch (err: unknown) {
        const apiErr = err as { message?: string; data?: { sandbox?: SandboxState } };
        if (apiErr.data?.sandbox) {
          setState((prev) =>
            prev ? { ...prev, sandbox: apiErr.data!.sandbox! } : prev
          );
        }
        const msg = apiErr.message ?? 'Failed to add sandbox rule.';
        pushToast('warning', 'Blocked by Landlock policy', msg);
        return msg;
      }
    },
    [pushToast]
  );

  const removeSandboxPath = useCallback(
    async (id: string) => {
      const data = await runMutation(
        () =>
          requestJson<{ sandbox: SandboxState }>(`/api/sandbox/paths/${id}`, {
            method: 'DELETE',
          }),
        'Failed to remove sandbox path'
      );
      if (data) {
        setState((prev) => (prev ? { ...prev, sandbox: data.sandbox } : prev));
        pushToast('info', 'Sandbox path rule removed');
      }
    },
    [pushToast, runMutation]
  );

  const toggleOutbound = useCallback(
    async (enabled: boolean) => {
      const data = await runMutation(
        () =>
          requestJson<{ sandbox: SandboxState }>('/api/sandbox/network', {
            method: 'PATCH',
            body: JSON.stringify({ outboundNetworkEnabled: enabled }),
          }),
        'Failed to toggle outbound network'
      );
      if (data) {
        setState((prev) => (prev ? { ...prev, sandbox: data.sandbox } : prev));
        pushToast(
          enabled ? 'warning' : 'success',
          enabled ? 'Outbound HTTP permitted' : 'Outbound HTTP blocked'
        );
      }
    },
    [pushToast, runMutation]
  );

  const saveMemory = useCallback(
    async (name: MemoryFile['name'], content: string) => {
      const data = await runMutation(
        () =>
          requestJson<{ memoryFiles: MemoryFile[]; auditLog: AuditEntry[] }>(
            '/api/memory/files',
            {
              method: 'PUT',
              body: JSON.stringify({ name, content }),
            }
          ),
        'Failed to save memory file'
      );
      if (data) {
        setState((prev) =>
          prev
            ? {
                ...prev,
                memoryFiles: data.memoryFiles,
                sandbox: { ...prev.sandbox, auditLog: data.auditLog },
              }
            : prev
        );
        pushToast('success', `${name} saved`);
      }
    },
    [pushToast, runMutation]
  );

  const manageSkill = useCallback(
    async (payload: {
      action: 'toggle' | 'apply_diff' | 'create';
      skillId?: string;
      name?: string;
      description?: string;
    }) => {
      const data = await runMutation(
        () =>
          requestJson<{ skills: SkillEntry[]; auditLog: AuditEntry[] }>(
            '/api/skills/manage',
            {
              method: 'POST',
              body: JSON.stringify(payload),
            }
          ),
        'Skill action failed'
      );
      if (data) {
        setState((prev) =>
          prev
            ? {
                ...prev,
                skills: data.skills,
                sandbox: { ...prev.sandbox, auditLog: data.auditLog },
              }
            : prev
        );
        if (payload.action === 'create') {
          pushToast('success', 'Project skill created', payload.name);
        } else if (payload.action === 'apply_diff') {
          pushToast('success', 'Skill patch applied');
        }
      }
    },
    [pushToast, runMutation]
  );

  const addMcpServer = useCallback(
    async (payload: {
      name: string;
      transport: 'stdio' | 'sse';
      endpoint: string;
    }) => {
      const data = await runMutation(
        () =>
          requestJson<{ mcpServers: McpServer[]; auditLog: AuditEntry[] }>(
            '/api/mcp/servers',
            {
              method: 'POST',
              body: JSON.stringify(payload),
            }
          ),
        'Failed to register MCP server'
      );
      if (data) {
        setState((prev) =>
          prev
            ? {
                ...prev,
                mcpServers: data.mcpServers,
                sandbox: { ...prev.sandbox, auditLog: data.auditLog },
              }
            : prev
        );
        pushToast('success', 'MCP server connected', payload.name);
      }
    },
    [pushToast, runMutation]
  );

  const pingMcpServer = useCallback(
    async (id: string) => {
      const data = await runMutation(
        () =>
          requestJson<{ mcpServers: McpServer[] }>(
            `/api/mcp/servers/${id}/ping`,
            { method: 'POST' }
          ),
        'Failed to ping MCP server'
      );
      if (data) {
        setState((prev) => (prev ? { ...prev, mcpServers: data.mcpServers } : prev));
        const srv = data.mcpServers.find((s) => s.id === id);
        if (srv) {
          pushToast('info', `${srv.name} responsive`, `${srv.latencyMs ?? 0}ms`);
        }
      }
    },
    [pushToast, runMutation]
  );

  const deleteMcpServer = useCallback(
    async (id: string) => {
      const data = await runMutation(
        () =>
          requestJson<{ mcpServers: McpServer[] }>(`/api/mcp/servers/${id}`, {
            method: 'DELETE',
          }),
        'Failed to remove MCP server'
      );
      if (data) {
        setState((prev) => (prev ? { ...prev, mcpServers: data.mcpServers } : prev));
        pushToast('info', 'MCP server removed');
      }
    },
    [pushToast, runMutation]
  );

  const addCronJob = useCallback(
    async (naturalPrompt: string, targetAgent: AgentRole): Promise<string | null> => {
      try {
        const data = await requestJson<{
          cronJobs: CronJob[];
          auditLog: AuditEntry[];
        }>('/api/cron/jobs', {
          method: 'POST',
          body: JSON.stringify({ naturalPrompt, targetAgent }),
        });
        setState((prev) =>
          prev
            ? {
                ...prev,
                cronJobs: data.cronJobs,
                sandbox: { ...prev.sandbox, auditLog: data.auditLog },
              }
            : prev
        );
        pushToast('success', 'Cron schedule created', `${targetAgent} assigned`);
        return null;
      } catch (err) {
        const msg = err instanceof Error ? err.message : 'Invalid schedule';
        return msg;
      }
    },
    [pushToast]
  );

  const toggleCronJob = useCallback(
    async (id: string) => {
      const data = await runMutation(
        () =>
          requestJson<{ cronJobs: CronJob[] }>(`/api/cron/jobs/${id}`, {
            method: 'PATCH',
          }),
        'Failed to update cron job'
      );
      if (data) {
        setState((prev) => (prev ? { ...prev, cronJobs: data.cronJobs } : prev));
      }
    },
    [runMutation]
  );

  const deleteCronJob = useCallback(
    async (id: string) => {
      const data = await runMutation(
        () =>
          requestJson<{ cronJobs: CronJob[] }>(`/api/cron/jobs/${id}`, {
            method: 'DELETE',
          }),
        'Failed to delete cron job'
      );
      if (data) {
        setState((prev) => (prev ? { ...prev, cronJobs: data.cronJobs } : prev));
        pushToast('info', 'Cron schedule removed');
      }
    },
    [pushToast, runMutation]
  );

  return {
    state,
    setState,
    loadError,
    retryLoad: fetchState,
    updatePreferences,
    createSession,
    selectSession,
    deleteSession,
    updateSessionMeta,
    resolveApproval,
    saveRouting,
    addProvider,
    toggleProvider,
    deleteProvider,
    testLatency,
    addSandboxPath,
    removeSandboxPath,
    toggleOutbound,
    saveMemory,
    manageSkill,
    addMcpServer,
    pingMcpServer,
    deleteMcpServer,
    addCronJob,
    toggleCronJob,
    deleteCronJob,
  };
}
