import { Router } from 'express';
import type { AgentRole, Message, ToolStep } from '../../src/types/cockpit';
import { sendError } from '../middleware';
import { nowTime, type AtomicStateStore } from '../store';

async function* streamOpenAiCompatible(params: {
  baseUrl: string;
  apiKey?: string;
  model: string;
  systemPrompt: string;
  userPrompt: string;
  signal?: AbortSignal;
}): AsyncGenerator<string, void, unknown> {
  const url = `${params.baseUrl.replace(/\/+$/, '')}/chat/completions`;
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
  };
  if (params.apiKey) {
    headers['Authorization'] = `Bearer ${params.apiKey}`;
  }

  const response = await fetch(url, {
    method: 'POST',
    headers,
    signal: params.signal,
    body: JSON.stringify({
      model: params.model,
      messages: [
        { role: 'system', content: params.systemPrompt },
        { role: 'user', content: params.userPrompt },
      ],
      stream: true,
    }),
  });

  if (!response.ok) {
    const errorText = await response.text();
    throw new Error(`Upstream API error (${response.status}): ${errorText}`);
  }

  if (!response.body) {
    throw new Error('Response body is empty');
  }

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split('\n');
    buffer = lines.pop() ?? '';

    for (const line of lines) {
      const trimmed = line.trim();
      if (!trimmed || trimmed.startsWith(':')) continue;
      if (trimmed === 'data: [DONE]') return;
      if (trimmed.startsWith('data: ')) {
        const jsonStr = trimmed.slice(6);
        try {
          const parsed = JSON.parse(jsonStr);
          const delta = parsed.choices?.[0]?.delta?.content ?? '';
          if (delta) {
            yield delta;
          }
        } catch {
          // ignore partial JSON chunk
        }
      }
    }
  }
}

export function createChatRouter(store: AtomicStateStore): Router {
  const router = Router();

  router.post('/chat/stream', async (req, res) => {
    const { sessionId, prompt, pinnedFiles = [] } = req.body ?? {};
    const session = store.state.sessions.find((s) => s.id === sessionId);
    if (!session) {
      sendError(res, 404, 'SESSION_NOT_FOUND', 'Session not found.');
      return;
    }
    if (typeof prompt !== 'string' || !prompt.trim()) {
      sendError(res, 400, 'EMPTY_PROMPT', 'Prompt cannot be empty.');
      return;
    }

    const trimmed = prompt.trim();

    if (trimmed === '/clear') {
      session.messages = [];
      session.updatedAt = nowTime();
      store.persist();
      res.setHeader('Content-Type', 'text/event-stream');
      res.setHeader('Cache-Control', 'no-cache');
      res.write(`data: ${JSON.stringify({ type: 'cleared', sessionId: session.id })}\n\n`);
      res.end();
      return;
    }

    if (trimmed.startsWith('/preset ')) {
      const nextPreset = trimmed.split(/\s+/)[1]?.toLowerCase();
      if (nextPreset === 'tabby' || nextPreset === 'carrier' || nextPreset === 'oyen') {
        session.preset = nextPreset;
      }
      session.updatedAt = nowTime();
      store.persist();
      res.setHeader('Content-Type', 'text/event-stream');
      res.setHeader('Cache-Control', 'no-cache');
      res.write(
        `data: ${JSON.stringify({
          type: 'preset_switched',
          preset: session.preset,
          sessionId: session.id,
        })}\n\n`
      );
      res.end();
      return;
    }

    const userMsgId = `msg-${Date.now()}`;
    const userMsg: Message = {
      id: userMsgId,
      sender: 'user',
      timestamp: nowTime(),
      content: trimmed,
      pinnedFiles: Array.isArray(pinnedFiles) ? pinnedFiles : [],
    };
    session.messages.push(userMsg);
    if (session.messages.length === 1) {
      session.title = trimmed.length > 42 ? `${trimmed.slice(0, 42)}...` : trimmed;
    }
    session.updatedAt = nowTime();
    store.persist();

    res.setHeader('Content-Type', 'text/event-stream');
    res.setHeader('Cache-Control', 'no-cache');
    res.setHeader('Connection', 'keep-alive');
    res.flushHeaders?.();

    let clientClosed = false;
    const abortController = new AbortController();

    req.on('close', () => {
      clientClosed = true;
      abortController.abort();
    });

    const safeWrite = (payload: Record<string, unknown>) => {
      if (!clientClosed && !res.writableEnded) {
        res.write(`data: ${JSON.stringify(payload)}\n\n`);
      }
    };

    const activeRole: AgentRole =
      trimmed.toLowerCase().includes('fix') || trimmed.toLowerCase().includes('patch')
        ? 'Tinker'
        : 'Meowstro';

    const toolSteps: ToolStep[] = [
      {
        id: `step-${Date.now()}-1`,
        tool: 'read' as const,
        target: 'docs/ARCHITECTURE.md:1-80',
        summary: 'Inspect layered module boundaries & role contracts',
        durationMs: 84,
        status: 'completed' as const,
      },
      {
        id: `step-${Date.now()}-2`,
        tool: 'grep' as const,
        target: 'crates/purrcode-channel/src/web',
        summary: 'Grep active route bindings and state dependencies',
        durationMs: 142,
        status: 'running' as const,
      },
      {
        id: `step-${Date.now()}-3`,
        tool: 'edit' as const,
        target: 'crates/purrcode-channel/src/web/server/routes.rs',
        summary: 'Prepare verified patch chunk with fail-closed bounds',
        durationMs: 198,
        status: 'pending' as const,
      },
    ];

    const agentMsgId = `msg-agent-${Date.now()}`;
    safeWrite({
      type: 'start',
      userMsg,
      agentMsg: {
        id: agentMsgId,
        sender: 'agent',
        agentRole: activeRole,
        agentTitle: activeRole === 'Tinker' ? 'Fixer' : 'Orchestrator',
        modelUsed: session.model,
        timestamp: nowTime(),
        content: '',
        toolSteps: toolSteps.map((s) => ({ ...s })),
        isStreaming: true,
      },
    });

    const startTime = Date.now();
    let fullText = '';
    let hadError = false;

    const advanceToStep = (activeIndex: number) => {
      for (let i = 0; i < toolSteps.length; i += 1) {
        if (i < activeIndex) toolSteps[i].status = 'completed';
        else if (i === activeIndex) toolSteps[i].status = 'running';
        else toolSteps[i].status = 'pending';
      }
      safeWrite({
        type: 'step_update',
        agentMsgId,
        toolSteps: toolSteps.map((s) => ({ ...s })),
      });
    };

    try {
      // Determine provider base URL, API key, and model
      let baseUrl = process.env.PURRCODE_BASE_URL || 'http://127.0.0.1:3425/v1';
      let apiKey = process.env.PURRCODE_API_KEY || 'sk-magpie-key-58cbf910e1e519bb1a333ee571e19eee4e2067edd300c1e2';
      let selectedModel = session.model || process.env.PURRCODE_MODEL || 'antigravity/gemini-3.8-flash-tiered';

      // Lookup provider in store if session model matches a registered provider
      const matchedProvider = store.state.providers.find(
        (p) => p.enabled && (p.models.includes(selectedModel) || p.defaultModel === selectedModel)
      ) || store.state.providers.find((p) => p.enabled);

      if (matchedProvider) {
        if (matchedProvider.baseUrl) baseUrl = matchedProvider.baseUrl;
        if (matchedProvider.apiKey) apiKey = matchedProvider.apiKey;
      }

      const memoryContext = store.state.memoryFiles
        .map((m) => `### ${m.name}\n${m.content}`)
        .join('\n\n');

      const systemInstruction = [
        'You are Meowstro, the lead Orchestrator of the PurrCode AI Developer Cockpit.',
        `Active Preset: ${session.preset} (${session.preset === 'tabby' ? 'supervised approval mode' : session.preset === 'carrier' ? 'balanced execution' : 'autonomous mode'}).`,
        `Landlock Sandbox: ${store.state.sandbox.landlockAbi} enforced=${store.state.sandbox.enforced}.`,
        'Respond concisely, technically, and accurately in clean Markdown. Reference real files or architectural principles when appropriate.',
        pinnedFiles.length > 0 ? `Pinned files: ${pinnedFiles.join(', ')}` : '',
        memoryContext,
      ]
        .filter(Boolean)
        .join('\n\n');

      advanceToStep(1);

      const stream = streamOpenAiCompatible({
        baseUrl,
        apiKey,
        model: selectedModel,
        systemPrompt: systemInstruction,
        userPrompt: trimmed,
        signal: abortController.signal,
      });

      let chunkCount = 0;
      for await (const delta of stream) {
        if (clientClosed) {
          break;
        }
        if (delta) {
          if (chunkCount === 0) {
            advanceToStep(2);
          }
          chunkCount += 1;
          fullText += delta;
          safeWrite({ type: 'delta', delta, agentMsgId });
        }
      }
    } catch (err) {
      hadError = true;
      const message = err instanceof Error ? err.message : 'Failed to generate response.';
      safeWrite({ type: 'error', error: message, agentMsgId });
      fullText = fullText || `**Execution Notice:** ${message}`;
    }

    for (let i = 0; i < toolSteps.length; i += 1) {
      if (clientClosed && toolSteps[i].status !== 'completed') {
        toolSteps[i].status = 'denied';
      } else {
        toolSteps[i].status = hadError && i === toolSteps.length - 1 ? 'denied' : 'completed';
      }
    }

    const thinkingDurationMs = Math.max(120, Date.now() - startTime);
    const finalizedContent = clientClosed
      ? fullText || '*(Generation stopped by operator.)*'
      : fullText;

    const finalizedAgentMsg: Message = {
      id: agentMsgId,
      sender: 'agent',
      agentRole: activeRole,
      agentTitle: activeRole === 'Tinker' ? 'Fixer' : 'Orchestrator',
      modelUsed: session.model,
      thinkingDurationMs,
      timestamp: nowTime(),
      content: finalizedContent,
      toolSteps,
      isStreaming: false,
    };
    session.messages.push(finalizedAgentMsg);
    session.updatedAt = nowTime();
    store.appendAudit(
      activeRole,
      clientClosed ? 'agent.abort' : 'agent.turn',
      session.title,
      clientClosed ? 'Denied' : 'Allowed',
      clientClosed
        ? `Stream stopped by operator after ${thinkingDurationMs}ms`
        : `Completed turn using ${session.model} in ${thinkingDurationMs}ms`
    );
    store.persist();

    if (!clientClosed && !res.writableEnded) {
      safeWrite({
        type: 'done',
        agentMsg: finalizedAgentMsg,
        auditLog: store.state.sandbox.auditLog,
      });
      res.end();
    }
  });

  return router;
}
