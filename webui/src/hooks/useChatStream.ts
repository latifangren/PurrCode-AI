import { useCallback, useEffect, useRef, useState } from 'react';
import type { CockpitState, Message, ToastTone, ToolStatus, ToolStep } from '../types/cockpit';

interface UseChatStreamOptions {
  activeSessionId: string | undefined;
  setState: React.Dispatch<React.SetStateAction<CockpitState | null>>;
  pushToast: (tone: ToastTone, title: string, detail?: string) => void;
  onCelebrate: () => void;
}

function normalizeToolName(tool?: string): 'read' | 'grep' | 'edit' | 'bash' | 'mcp' {
  if (!tool) return 'bash';
  const lower = tool.toLowerCase();
  if (lower.includes('read') || lower.includes('glob') || lower.includes('find')) return 'read';
  if (lower.includes('grep')) return 'grep';
  if (lower.includes('edit') || lower.includes('write') || lower.includes('patch')) return 'edit';
  if (lower.includes('mcp')) return 'mcp';
  return 'bash';
}

export function useChatStream({
  activeSessionId,
  setState,
  pushToast,
  onCelebrate,
}: UseChatStreamOptions) {
  const [streamingMessage, setStreamingMessage] = useState<Message | null>(null);
  const [streamError, setStreamError] = useState<string | null>(null);
  const [streamElapsedMs, setStreamElapsedMs] = useState<number>(0);
  const abortControllerRef = useRef<AbortController | null>(null);
  const streamStartRef = useRef<number | null>(null);

  useEffect(() => {
    if (!streamingMessage) {
      setStreamElapsedMs(0);
      streamStartRef.current = null;
      return;
    }
    if (streamStartRef.current === null) {
      streamStartRef.current = Date.now();
    }
    const interval = setInterval(() => {
      if (streamStartRef.current !== null) {
        setStreamElapsedMs(Date.now() - streamStartRef.current);
      }
    }, 100);
    return () => clearInterval(interval);
  }, [Boolean(streamingMessage)]);

  const stopStream = useCallback(() => {
    if (abortControllerRef.current) {
      abortControllerRef.current.abort();
      abortControllerRef.current = null;
    }
  }, []);

  const sendPrompt = useCallback(
    async (prompt: string, pinnedFiles: string[]) => {
      if (!activeSessionId) return;
      setStreamError(null);

      const controller = new AbortController();
      abortControllerRef.current = controller;
      streamStartRef.current = Date.now();

      const timestamp = new Date().toLocaleTimeString([], {
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
      });

      const userMsg: Message = {
        id: `msg-u-${Date.now()}`,
        sender: 'user',
        timestamp,
        content: prompt,
        pinnedFiles: pinnedFiles.length > 0 ? pinnedFiles : undefined,
      };

      const agentMsg: Message = {
        id: `msg-a-${Date.now()}`,
        sender: 'agent',
        agentRole: 'Meowstro',
        agentTitle: 'Orchestrator',
        modelUsed: 'antigravity/gemini-3.8-flash-tiered',
        timestamp,
        content: '',
        isStreaming: true,
        toolSteps: [],
      };

      let latestStreamingMsg: Message | null = agentMsg;
      setStreamingMessage(agentMsg);

      setState((prev) =>
        prev
          ? {
              ...prev,
              sessions: prev.sessions.map((s) =>
                s.id === activeSessionId
                  ? {
                      ...s,
                      messages: [...s.messages, userMsg],
                      title:
                        s.messages.length === 0
                          ? prompt.slice(0, 42) + (prompt.length > 42 ? '...' : '')
                          : s.title,
                    }
                  : s
              ),
            }
          : prev
      );

      try {
        // Subscribe to real-time session SSE event stream
        const eventsUrl = `/api/events?session=${encodeURIComponent(activeSessionId)}`;
        const eventsResponse = await fetch(eventsUrl, {
          signal: controller.signal,
          headers: { Accept: 'text/event-stream' },
        });

        if (!eventsResponse.ok || !eventsResponse.body) {
          throw new Error(`SSE stream connection failed (${eventsResponse.status})`);
        }

        // Dispatch prompt to backend Rust router
        const promptResponse = await fetch('/api/prompt', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          signal: controller.signal,
          body: JSON.stringify({
            session_id: activeSessionId,
            prompt,
          }),
        });

        if (!promptResponse.ok) {
          let errMsg = `Prompt dispatch failed (${promptResponse.status})`;
          try {
            const errJson = (await promptResponse.json()) as { error?: string };
            if (errJson.error) errMsg = errJson.error;
          } catch {
            // keep default error text
          }
          throw new Error(errMsg);
        }

        const reader = eventsResponse.body.getReader();
        const decoder = new TextDecoder();
        let buffer = '';

        while (true) {
          const { done, value } = await reader.read();
          if (done) break;
          buffer += decoder.decode(value, { stream: true });

          const lines = buffer.split('\n\n');
          buffer = lines.pop() ?? '';

          for (const block of lines) {
            const dataLine = block
              .split('\n')
              .find((line) => line.startsWith('data: '));
            if (!dataLine) continue;

            const rawJson = dataLine.slice(6).trim();
            if (!rawJson) continue;

            try {
              const ev = JSON.parse(rawJson);
              const evType = ev.type;
              const payload = ev.payload ?? ev;

              if (evType === 'TextChunk') {
                const text = payload.text ?? '';
                setStreamingMessage((prev) => {
                  if (!prev) return prev;
                  const next = { ...prev, content: prev.content + text };
                  latestStreamingMsg = next;
                  return next;
                });
              } else if (evType === 'ReasoningChunk') {
                // thinking chunks
              } else if (evType === 'ToolCallStarted') {
                setStreamingMessage((prev) => {
                  if (!prev) return prev;
                  const steps: ToolStep[] = prev.toolSteps ? [...prev.toolSteps] : [];
                  steps.push({
                    id: payload.call_id || `ts-${Date.now()}`,
                    tool: normalizeToolName(payload.tool),
                    target:
                      typeof payload.arguments === 'string'
                        ? payload.arguments
                        : JSON.stringify(payload.arguments ?? {}),
                    summary: `Executing ${payload.tool || 'tool'}`,
                    durationMs: 0,
                    status: 'running',
                  });
                  const next: Message = { ...prev, toolSteps: steps };
                  latestStreamingMsg = next;
                  return next;
                });
              } else if (evType === 'ToolCallApprovalRequested') {
                setStreamingMessage((prev) => {
                  if (!prev) return prev;
                  const next: Message = {
                    ...prev,
                    approval: {
                      id: payload.call_id || `apr-${Date.now()}`,
                      action: payload.description || 'Operation requires approval',
                      target: payload.tool || 'system',
                      riskLevel: 'high',
                      timeoutSeconds: 30,
                      status: 'pending',
                    },
                  };
                  latestStreamingMsg = next;
                  return next;
                });
              } else if (evType === 'ToolCallFinished') {
                setStreamingMessage((prev) => {
                  if (!prev) return prev;
                  const steps: ToolStep[] = (prev.toolSteps ?? []).map((step) =>
                    step.id === payload.call_id
                      ? {
                          ...step,
                          status: (payload.success ? 'completed' : 'denied') as ToolStatus,
                          durationMs: payload.duration_ms ?? 120,
                        }
                      : step
                  );
                  const next: Message = { ...prev, toolSteps: steps };
                  latestStreamingMsg = next;
                  return next;
                });
              } else if (evType === 'delta') {
                setStreamingMessage((prev) => {
                  if (!prev) return prev;
                  const next = { ...prev, content: prev.content + payload.delta };
                  latestStreamingMsg = next;
                  return next;
                });
              } else if (evType === 'step_update') {
                setStreamingMessage((prev) => {
                  if (!prev) return prev;
                  const next = { ...prev, toolSteps: payload.toolSteps };
                  latestStreamingMsg = next;
                  return next;
                });
              } else if (evType === 'TurnFinished' || evType === 'done') {
                const finalMsg: Message = latestStreamingMsg
                  ? {
                      ...latestStreamingMsg,
                      isStreaming: false,
                      thinkingDurationMs: streamStartRef.current
                        ? Math.max(120, Date.now() - streamStartRef.current)
                        : undefined,
                    }
                  : {
                      ...agentMsg,
                      isStreaming: false,
                      content: 'Turn completed.',
                    };

                setStreamingMessage(null);
                latestStreamingMsg = null;
                setState((prev) =>
                  prev
                    ? {
                        ...prev,
                        sessions: prev.sessions.map((s) =>
                          s.id === activeSessionId
                            ? { ...s, messages: [...s.messages, finalMsg] }
                            : s
                        ),
                      }
                    : prev
                );
                onCelebrate();
                return;
              } else if (evType === 'Error') {
                const errMsg =
                  payload.message || payload.error || 'Execution error encountered.';
                setStreamError(errMsg);
                pushToast('error', 'Execution Error', errMsg);
              }
            } catch {
              // Ignore invalid JSON on ping/heartbeat frames
            }
          }
        }
      } catch (err) {
        const isAbort = err instanceof DOMException && err.name === 'AbortError';
        if (isAbort && latestStreamingMsg) {
          const stoppedMsg: Message = {
            ...latestStreamingMsg,
            isStreaming: false,
            thinkingDurationMs: streamStartRef.current
              ? Math.max(120, Date.now() - streamStartRef.current)
              : undefined,
            content:
              latestStreamingMsg.content ||
              '*(Generation stopped by operator.)*',
            toolSteps: latestStreamingMsg.toolSteps?.map((step) =>
              step.status === 'completed' ? step : { ...step, status: 'denied' }
            ),
          };
          setState((prev) =>
            prev
              ? {
                  ...prev,
                  sessions: prev.sessions.map((s) =>
                    s.id === activeSessionId
                      ? { ...s, messages: [...s.messages, stoppedMsg] }
                      : s
                  ),
                }
              : prev
          );
          pushToast(
            'warning',
            'Generation stopped',
            'Partial progress preserved in timeline.'
          );
        } else if (!isAbort) {
          const msg =
            err instanceof Error ? err.message : 'Unknown streaming error';
          setStreamError(msg);
          pushToast('error', 'Streaming failed', msg);
        }
      } finally {
        setStreamingMessage(null);
        abortControllerRef.current = null;
        streamStartRef.current = null;
      }
    },
    [activeSessionId, onCelebrate, pushToast, setState]
  );

  return {
    streamingMessage,
    streamError,
    streamElapsedMs,
    isStreaming: Boolean(streamingMessage),
    sendPrompt,
    stopStream,
  };
}
