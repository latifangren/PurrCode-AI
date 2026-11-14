import { Router } from 'express';
import { resolveApprovalState } from '../../src/lib/cockpitLogic';
import type { PresetMode, Session } from '../../src/types/cockpit';
import { sendError } from '../middleware';
import { nowTime, type AtomicStateStore } from '../store';

export function createSessionsRouter(store: AtomicStateStore): Router {
  const router = Router();

  router.get('/health', (_req, res) => {
    res.json(store.getHealthSummary());
  });

  router.get('/state', (_req, res) => {
    res.json(store.state);
  });

  router.post('/sessions', (req, res) => {
    const title =
      typeof req.body?.title === 'string' && req.body.title.trim()
        ? req.body.title.trim()
        : `Session #${store.state.sessions.length + 10}`;

    const newSession: Session = {
      id: `session-${Date.now()}`,
      title,
      branch: 'dev-muse',
      preset: 'tabby',
      model: 'gemini-4-hydrogen',
      updatedAt: nowTime(),
      messages: [],
    };

    store.state.sessions.unshift(newSession);
    store.state.activeSessionId = newSession.id;
    store.persist();
    res.status(201).json({ session: newSession, activeSessionId: newSession.id });
  });

  router.patch('/sessions/:id', (req, res) => {
    const session = store.state.sessions.find((s) => s.id === req.params.id);
    if (!session) {
      sendError(res, 404, 'SESSION_NOT_FOUND', 'Session not found.');
      return;
    }

    if (req.body?.activate) {
      store.state.activeSessionId = session.id;
    }
    if (typeof req.body?.preset === 'string') {
      if (!['tabby', 'carrier', 'oyen'].includes(req.body.preset)) {
        sendError(res, 400, 'INVALID_PRESET', 'Preset must be tabby, carrier, or oyen.');
        return;
      }
      session.preset = req.body.preset as PresetMode;
    }
    if (typeof req.body?.model === 'string' && req.body.model.trim()) {
      session.model = req.body.model.trim();
    }
    if (typeof req.body?.title === 'string' && req.body.title.trim()) {
      session.title = req.body.title.trim();
    }

    session.updatedAt = nowTime();
    store.persist();
    res.json({ session, activeSessionId: store.state.activeSessionId });
  });

  router.delete('/sessions/:id', (req, res) => {
    if (store.state.sessions.length <= 1) {
      sendError(res, 400, 'LAST_SESSION_LOCKED', 'Cannot delete the last remaining session.');
      return;
    }
    const exists = store.state.sessions.some((s) => s.id === req.params.id);
    if (!exists) {
      sendError(res, 404, 'SESSION_NOT_FOUND', 'Session not found.');
      return;
    }
    store.state.sessions = store.state.sessions.filter((s) => s.id !== req.params.id);
    if (store.state.activeSessionId === req.params.id) {
      store.state.activeSessionId = store.state.sessions[0].id;
    }
    store.persist();
    res.json({
      sessions: store.state.sessions,
      activeSessionId: store.state.activeSessionId,
    });
  });

  router.post('/approvals/:id', (req, res) => {
    const decision = req.body?.decision as 'approved' | 'denied' | 'expired';
    if (!['approved', 'denied', 'expired'].includes(decision)) {
      sendError(res, 400, 'INVALID_DECISION', 'Decision must be approved, denied, or expired.');
      return;
    }

    let updated = false;
    for (const session of store.state.sessions) {
      for (const msg of session.messages) {
        if (msg.approval && msg.approval.id === req.params.id) {
          msg.approval = resolveApprovalState(msg.approval, decision);
          if (msg.diff) {
            msg.diff.status = decision === 'approved' ? 'accepted' : 'rejected';
          }
          if (msg.toolSteps) {
            for (const step of msg.toolSteps) {
              if (step.status === 'pending') {
                step.status = decision === 'approved' ? 'completed' : 'denied';
              }
            }
          }
          store.appendAudit(
            msg.agentRole ?? 'Tinker',
            'fs.apply_patch',
            msg.approval.target,
            decision === 'approved' ? 'Allowed' : 'Denied',
            decision === 'approved'
              ? 'Supervised patch approved by operator'
              : decision === 'expired'
              ? 'Auto-denied by fail-closed 30s timeout'
              : 'Patch rejected by operator'
          );
          updated = true;
        }
      }
    }

    if (!updated) {
      sendError(res, 404, 'APPROVAL_NOT_FOUND', 'Approval request not found.');
      return;
    }

    store.persist();
    res.json({ sandbox: store.state.sandbox, sessions: store.state.sessions });
  });

  return router;
}
