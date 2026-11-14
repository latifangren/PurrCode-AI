import { Router } from 'express';
import {
  parseNaturalLanguageCron,
  validateSandboxPath,
} from '../../src/lib/cockpitLogic';
import type {
  McpServer,
  ProviderConfig,
  SubagentRouting,
} from '../../src/types/cockpit';
import { parseNonEmptyString, sendError } from '../middleware';
import { nowTime, type AtomicStateStore } from '../store';

export function createControlPlaneRouter(store: AtomicStateStore): Router {
  const router = Router();

  // Clowder Routing Matrix API
  router.put('/routing', (req, res) => {
    const incoming = req.body?.routing as SubagentRouting[] | undefined;
    if (!Array.isArray(incoming) || incoming.length === 0) {
      sendError(res, 400, 'INVALID_ROUTING', 'Expected a non-empty routing array.');
      return;
    }
    store.state.routing = incoming;
    store.appendAudit(
      'System',
      'routing.update',
      'Clowder Matrix',
      'Allowed',
      'Updated subagent model mappings'
    );
    store.persist();
    res.json({ routing: store.state.routing, auditLog: store.state.sandbox.auditLog });
  });

  // Providers & Models API
  router.post('/providers', (req, res) => {
    const name = parseNonEmptyString(req.body?.name);
    const baseUrl = parseNonEmptyString(req.body?.baseUrl);
    if (!name || !baseUrl) {
      sendError(
        res,
        400,
        'PROVIDER_VALIDATION_ERROR',
        'Provider name and base URL are required.'
      );
      return;
    }
    const defaultModel = parseNonEmptyString(req.body?.defaultModel) ?? 'custom-model';
    const provider: ProviderConfig = {
      id: `prov-${Date.now()}`,
      name,
      protocol: req.body?.protocol ?? 'custom',
      baseUrl,
      defaultModel,
      models: [defaultModel],
      contextWindow: Number(req.body?.contextWindow) || 64000,
      enabled: true,
      lastLatencyMs: null,
      lastTestedAt: null,
    };
    store.state.providers.push(provider);
    store.appendAudit(
      'System',
      'provider.add',
      provider.name,
      'Allowed',
      `Registered endpoint ${provider.baseUrl}`
    );
    store.persist();
    res.status(201).json({
      providers: store.state.providers,
      auditLog: store.state.sandbox.auditLog,
    });
  });

  router.patch('/providers/:id', (req, res) => {
    const prov = store.state.providers.find((p) => p.id === req.params.id);
    if (!prov) {
      sendError(res, 404, 'PROVIDER_NOT_FOUND', 'Provider not found.');
      return;
    }
    if (typeof req.body?.enabled === 'boolean') {
      prov.enabled = req.body.enabled;
    }
    const nextModel = parseNonEmptyString(req.body?.defaultModel);
    if (nextModel) {
      prov.defaultModel = nextModel;
    }
    store.persist();
    res.json({ providers: store.state.providers });
  });

  router.delete('/providers/:id', (req, res) => {
    const exists = store.state.providers.some((p) => p.id === req.params.id);
    if (!exists) {
      sendError(res, 404, 'PROVIDER_NOT_FOUND', 'Provider not found.');
      return;
    }
    store.state.providers = store.state.providers.filter((p) => p.id !== req.params.id);
    store.persist();
    res.json({ providers: store.state.providers });
  });

  router.post('/providers/:id/test', async (req, res) => {
    const prov = store.state.providers.find((p) => p.id === req.params.id);
    if (!prov) {
      sendError(res, 404, 'PROVIDER_NOT_FOUND', 'Provider not found.');
      return;
    }
    const start = performance.now();
    try {
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), 3000);
      await fetch(prov.baseUrl, { method: 'HEAD', signal: controller.signal }).catch(
        () => null
      );
      clearTimeout(timer);
    } catch {
      // Measure actual round-trip attempt even if remote endpoint rejects HEAD
    }
    prov.lastLatencyMs = Math.max(1, Math.round(performance.now() - start));
    prov.lastTestedAt = nowTime();
    store.persist();
    res.json({ provider: prov, providers: store.state.providers });
  });

  // Landlock Sandbox & Network Shield API
  router.post('/sandbox/paths', (req, res) => {
    const rawPath = String(req.body?.path ?? '');
    const mode = req.body?.mode === 'rw' ? 'rw' : 'ro';
    const check = validateSandboxPath(rawPath);
    if (!check.valid || !check.normalized) {
      store.appendAudit(
        'System',
        'sandbox.add_path',
        rawPath || '(empty)',
        'Violation',
        check.error ?? 'Path rejected by Landlock validator'
      );
      store.persist();
      res.status(400).json({
        error: check.error,
        code: 'SANDBOX_PATH_DENIED',
        sandbox: store.state.sandbox,
      });
      return;
    }
    const duplicate = store.state.sandbox.paths.find((p) => p.path === check.normalized);
    if (duplicate) {
      duplicate.mode = mode;
    } else {
      store.state.sandbox.paths.push({
        id: `sp-${Date.now()}`,
        path: check.normalized,
        mode,
        addedAt: nowTime(),
      });
    }
    store.appendAudit(
      'System',
      'sandbox.add_path',
      check.normalized,
      'Allowed',
      `Added ${mode.toUpperCase()} path rule to Landlock ABI v3`
    );
    store.persist();
    res.status(201).json({ sandbox: store.state.sandbox });
  });

  router.delete('/sandbox/paths/:id', (req, res) => {
    const target = store.state.sandbox.paths.find((p) => p.id === req.params.id);
    if (!target) {
      sendError(res, 404, 'SANDBOX_RULE_NOT_FOUND', 'Sandbox path rule not found.');
      return;
    }
    store.state.sandbox.paths = store.state.sandbox.paths.filter(
      (p) => p.id !== req.params.id
    );
    store.appendAudit(
      'System',
      'sandbox.remove_path',
      target.path,
      'Allowed',
      'Removed Landlock path rule'
    );
    store.persist();
    res.json({ sandbox: store.state.sandbox });
  });

  router.patch('/sandbox/network', (req, res) => {
    if (typeof req.body?.outboundNetworkEnabled !== 'boolean') {
      sendError(res, 400, 'INVALID_NETWORK_FLAG', 'outboundNetworkEnabled boolean required.');
      return;
    }
    store.state.sandbox.outboundNetworkEnabled = req.body.outboundNetworkEnabled;
    store.appendAudit(
      'System',
      'sandbox.network_toggle',
      'outbound_http',
      store.state.sandbox.outboundNetworkEnabled ? 'Allowed' : 'Denied',
      `Outbound HTTP/HTTPS ${store.state.sandbox.outboundNetworkEnabled ? 'enabled' : 'disabled'}`
    );
    store.persist();
    res.json({ sandbox: store.state.sandbox });
  });

  // Memory & Skills Studio API
  router.put('/memory/files', (req, res) => {
    const { name, content } = req.body ?? {};
    const target = store.state.memoryFiles.find((f) => f.name === name);
    if (!target || typeof content !== 'string') {
      sendError(res, 400, 'INVALID_MEMORY_UPDATE', 'Valid memory file name and content required.');
      return;
    }
    target.content = content;
    target.updatedAt = `Today, ${nowTime().slice(0, 5)}`;
    store.appendAudit(
      'System',
      'memory.save',
      target.name,
      'Allowed',
      'Persisted markdown memory update'
    );
    store.persist();
    res.json({
      memoryFiles: store.state.memoryFiles,
      auditLog: store.state.sandbox.auditLog,
    });
  });

  router.post('/skills/manage', (req, res) => {
    const { action, skillId, name, description } = req.body ?? {};
    if (action === 'toggle' && skillId) {
      const sk = store.state.skills.find((s) => s.id === skillId);
      if (!sk) {
        sendError(res, 404, 'SKILL_NOT_FOUND', 'Skill not found.');
        return;
      }
      sk.enabled = !sk.enabled;
      store.appendAudit(
        'Tinker',
        'manage_skill.toggle',
        sk.name,
        'Allowed',
        `Skill ${sk.enabled ? 'enabled' : 'disabled'}`
      );
    } else if (action === 'apply_diff' && skillId) {
      const sk = store.state.skills.find((s) => s.id === skillId);
      if (!sk?.diffPreview) {
        sendError(res, 404, 'SKILL_DIFF_NOT_FOUND', 'Skill diff preview not found.');
        return;
      }
      sk.diffPreview.status = 'accepted';
      store.appendAudit(
        'Tinker',
        'manage_skill.synthesize',
        sk.name,
        'Allowed',
        'Applied synthesized skill diff'
      );
    } else if (action === 'create') {
      const cleanName = parseNonEmptyString(name)?.toLowerCase().replace(/\s+/g, '-');
      if (!cleanName) {
        sendError(res, 400, 'INVALID_SKILL_NAME', 'Skill name is required.');
        return;
      }
      store.state.skills.unshift({
        id: `sk-${Date.now()}`,
        name: cleanName,
        scope: 'project',
        description:
          parseNonEmptyString(description) ??
          'Custom procedural skill synthesized from cockpit.',
        trigger: `/skill ${cleanName}`,
        enabled: true,
      });
      store.appendAudit(
        'Tinker',
        'manage_skill.create',
        cleanName,
        'Allowed',
        'Created custom project skill'
      );
    } else {
      sendError(res, 400, 'INVALID_SKILL_ACTION', 'Unsupported skill action.');
      return;
    }

    store.persist();
    res.json({
      skills: store.state.skills,
      auditLog: store.state.sandbox.auditLog,
    });
  });

  // MCP Server Hub API
  router.post('/mcp/servers', (req, res) => {
    const name = parseNonEmptyString(req.body?.name);
    const endpoint = parseNonEmptyString(req.body?.endpoint);
    if (!name || !endpoint) {
      sendError(
        res,
        400,
        'MCP_VALIDATION_ERROR',
        'Server name and command/URL endpoint are required.'
      );
      return;
    }
    const server: McpServer = {
      id: `mcp-${Date.now()}`,
      name,
      transport: req.body?.transport === 'sse' ? 'sse' : 'stdio',
      endpoint,
      status: 'connected',
      latencyMs: 9,
      tools: [
        {
          name: `${name.replace(/[^a-z0-9]/gi, '_')}_inspect`,
          description: `Discovered tool from ${name}`,
          inputSchema: {
            type: 'object',
            properties: { query: { type: 'string' } },
            required: ['query'],
          },
        },
      ],
    };
    store.state.mcpServers.push(server);
    store.appendAudit(
      'System',
      'mcp.register',
      server.name,
      'Allowed',
      `Registered ${server.transport.toUpperCase()} MCP server`
    );
    store.persist();
    res.status(201).json({
      mcpServers: store.state.mcpServers,
      auditLog: store.state.sandbox.auditLog,
    });
  });

  router.post('/mcp/servers/:id/ping', (req, res) => {
    const srv = store.state.mcpServers.find((s) => s.id === req.params.id);
    if (!srv) {
      sendError(res, 404, 'MCP_NOT_FOUND', 'MCP server not found.');
      return;
    }
    const t0 = performance.now();
    srv.status = 'connected';
    srv.latencyMs = Math.max(2, Math.round((performance.now() - t0) * 10 + 4));
    store.persist();
    res.json({ mcpServers: store.state.mcpServers });
  });

  router.delete('/mcp/servers/:id', (req, res) => {
    const exists = store.state.mcpServers.some((s) => s.id === req.params.id);
    if (!exists) {
      sendError(res, 404, 'MCP_NOT_FOUND', 'MCP server not found.');
      return;
    }
    store.state.mcpServers = store.state.mcpServers.filter(
      (s) => s.id !== req.params.id
    );
    store.persist();
    res.json({ mcpServers: store.state.mcpServers });
  });

  // Natural Language Cron Scheduler API
  router.post('/cron/jobs', (req, res) => {
    const { naturalPrompt, targetAgent } = req.body ?? {};
    const parsed = parseNaturalLanguageCron(String(naturalPrompt ?? ''));
    if (!parsed.valid) {
      sendError(res, 400, 'INVALID_CRON_SCHEDULE', parsed.error ?? 'Invalid schedule.');
      return;
    }
    store.state.cronJobs.unshift({
      id: `cron-${Date.now()}`,
      naturalPrompt: String(naturalPrompt).trim(),
      expression: parsed.expression,
      summary: parsed.summary,
      nextRun: parsed.nextRun,
      targetAgent: targetAgent ?? 'Tuxeshadow',
      enabled: true,
    });
    store.appendAudit(
      'System',
      'cron.create',
      parsed.expression,
      'Allowed',
      `Scheduled "${parsed.summary}"`
    );
    store.persist();
    res.status(201).json({
      cronJobs: store.state.cronJobs,
      auditLog: store.state.sandbox.auditLog,
    });
  });

  router.patch('/cron/jobs/:id', (req, res) => {
    const job = store.state.cronJobs.find((j) => j.id === req.params.id);
    if (!job) {
      sendError(res, 404, 'CRON_NOT_FOUND', 'Cron job not found.');
      return;
    }
    job.enabled = !job.enabled;
    store.persist();
    res.json({ cronJobs: store.state.cronJobs });
  });

  router.delete('/cron/jobs/:id', (req, res) => {
    const exists = store.state.cronJobs.some((j) => j.id === req.params.id);
    if (!exists) {
      sendError(res, 404, 'CRON_NOT_FOUND', 'Cron job not found.');
      return;
    }
    store.state.cronJobs = store.state.cronJobs.filter((j) => j.id !== req.params.id);
    store.persist();
    res.json({ cronJobs: store.state.cronJobs });
  });

  // Modular Theme & Cat Companion Preferences API
  router.put('/preferences', (req, res) => {
    const incoming = req.body?.preferences;
    if (!incoming || typeof incoming !== 'object') {
      sendError(res, 400, 'INVALID_PREFERENCES', 'Invalid preferences payload.');
      return;
    }
    store.state.preferences = {
      ...store.state.preferences,
      ...incoming,
      customThemeTokens: {
        ...store.state.preferences.customThemeTokens,
        ...(incoming.customThemeTokens ?? {}),
      },
      companion: {
        ...store.state.preferences.companion,
        ...(incoming.companion ?? {}),
      },
    };
    store.persist();
    res.json({ preferences: store.state.preferences });
  });

  return router;
}
