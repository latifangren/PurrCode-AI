import test from 'node:test';
import assert from 'node:assert/strict';
import {
  computeContextMetrics,
  computeTaskProgress,
  filterAuditEntries,
  formatElapsedMs,
  matchSlashCommands,
  parseNaturalLanguageCron,
  pickCompanionReactionQuip,
  resolveApprovalState,
  validateSandboxPath,
} from './cockpitLogic';
import type { ApprovalRequest, AuditEntry, Message } from '../types/cockpit';

test('validateSandboxPath rejects traversal and sensitive paths', () => {
  assert.equal(validateSandboxPath('').valid, false);
  assert.equal(validateSandboxPath('/etc/passwd').valid, false);
  assert.equal(validateSandboxPath('/home/dev/.ssh/id_rsa').valid, false);
  assert.equal(validateSandboxPath('/workspace/../etc').valid, false);
  assert.equal(validateSandboxPath('/').valid, false);

  const ok = validateSandboxPath('/workspace/purrcode/crates//');
  assert.equal(ok.valid, true);
  assert.equal(ok.normalized, '/workspace/purrcode/crates');
});

test('parseNaturalLanguageCron parses common English and cron syntax', () => {
  const min = parseNaturalLanguageCron('every 15 minutes');
  assert.equal(min.valid, true);
  assert.equal(min.expression, '*/15 * * * *');

  const weekday = parseNaturalLanguageCron('weekdays at 8:30am');
  assert.equal(weekday.valid, true);
  assert.equal(weekday.expression, '30 8 * * 1-5');

  const daily = parseNaturalLanguageCron('daily at 21:00');
  assert.equal(daily.valid, true);
  assert.equal(daily.expression, '0 21 * * *');

  const raw = parseNaturalLanguageCron('0 4 * * 0');
  assert.equal(raw.valid, true);
  assert.equal(raw.expression, '0 4 * * 0');

  const bad = parseNaturalLanguageCron('sometime maybe');
  assert.equal(bad.valid, false);
});

test('matchSlashCommands filters autocomplete candidates', () => {
  assert.equal(matchSlashCommands('hello').length, 0);
  const all = matchSlashCommands('/');
  assert.equal(all.length, 5);
  const filtered = matchSlashCommands('/pr');
  assert.equal(filtered.length, 1);
  assert.equal(filtered[0].command, '/preset');
});

test('computeContextMetrics calculates deterministic usage', () => {
  const msgs: Message[] = [
    {
      id: '1',
      sender: 'user',
      timestamp: '10:00:00',
      content: 'A'.repeat(3600),
    },
  ];
  const metrics = computeContextMetrics(msgs, 128000);
  assert.ok(metrics.usedTokens > 1000);
  assert.equal(metrics.status, 'nominal');
});

test('filterAuditEntries filters by outcome and search query', () => {
  const entries: AuditEntry[] = [
    {
      id: 'a1',
      timestamp: '10:00:01',
      actor: 'Tinker',
      operation: 'fs.write',
      target: 'src/routes.rs',
      outcome: 'Allowed',
      detail: 'Approved by user',
    },
    {
      id: 'a2',
      timestamp: '10:00:02',
      actor: 'Whisker',
      operation: 'fs.read',
      target: '/etc/shadow',
      outcome: 'Violation',
      detail: 'Landlock blocked',
    },
  ];
  assert.equal(filterAuditEntries(entries, 'Violation', '').length, 1);
  assert.equal(filterAuditEntries(entries, 'All', 'routes').length, 1);
  assert.equal(filterAuditEntries(entries, 'Allowed', 'shadow').length, 0);
});

test('resolveApprovalState transitions pending approvals idempotently', () => {
  const pending: ApprovalRequest = {
    id: 'apr-1',
    action: 'Apply patch',
    target: 'src/routes.rs',
    riskLevel: 'high',
    timeoutSeconds: 30,
    status: 'pending',
  };
  const approved = resolveApprovalState(pending, 'approved');
  assert.equal(approved.status, 'approved');
  assert.ok(approved.resolvedAt);

  const second = resolveApprovalState(approved, 'denied');
  assert.equal(second.status, 'approved');
});

test('computeTaskProgress tracks multi-step AI tasks accurately across states', () => {
  assert.equal(computeTaskProgress([], false), null);

  // Single completed historical step stays hidden
  const singleDone: Message[] = [
    { id: 'u1', sender: 'user', timestamp: '10:00:00', content: 'Check policy' },
    {
      id: 'a1',
      sender: 'agent',
      agentRole: 'Whisker',
      timestamp: '10:00:02',
      content: 'Done.',
      toolSteps: [
        { id: 's1', tool: 'grep', target: 'policy.rs', summary: 'Checked', durationMs: 20, status: 'completed' },
      ],
    },
  ];
  assert.equal(computeTaskProgress(singleDone, false), null);

  // Multi-step task with 3 completed and 1 pending approval -> 75%
  const multiPending: Message[] = [
    { id: 'u1', sender: 'user', timestamp: '10:00:00', content: 'Refactor routes' },
    {
      id: 'a1',
      sender: 'agent',
      agentRole: 'Meowstro',
      timestamp: '10:00:01',
      content: 'Delegating',
      toolSteps: [
        { id: 's1', tool: 'read', target: 'routes.rs', summary: 'Read', durationMs: 20, status: 'completed' },
        { id: 's2', tool: 'grep', target: 'Router', summary: 'Grep', durationMs: 25, status: 'completed' },
      ],
    },
    {
      id: 'a2',
      sender: 'agent',
      agentRole: 'Tinker',
      timestamp: '10:00:02',
      content: 'Patched',
      toolSteps: [
        { id: 's3', tool: 'read', target: 'server.rs', summary: 'Read', durationMs: 18, status: 'completed' },
        { id: 's4', tool: 'edit', target: 'routes.rs', summary: 'Patch', durationMs: 90, status: 'pending' },
      ],
      approval: {
        id: 'apr-1',
        action: 'Apply patch',
        target: 'routes.rs',
        riskLevel: 'high',
        timeoutSeconds: 30,
        status: 'pending',
      },
    },
  ];
  const awaiting = computeTaskProgress(multiPending, false);
  assert.ok(awaiting);
  assert.equal(awaiting.percentage, 75);
  assert.equal(awaiting.status, 'awaiting-approval');
  assert.equal(awaiting.completedCount, 3);
  assert.equal(awaiting.totalSteps, 4);
  assert.equal(awaiting.activeActor, 'Tinker');

  // Streaming mid-task (1 completed, 1 running, 1 pending -> 1.5 / 3 = 50%)
  const streamingMsg: Message[] = [
    { id: 'u2', sender: 'user', timestamp: '10:05:00', content: 'Analyze' },
    {
      id: 'a3',
      sender: 'agent',
      agentRole: 'Meowstro',
      timestamp: '10:05:01',
      content: 'Working...',
      isStreaming: true,
      toolSteps: [
        { id: 's5', tool: 'read', target: 'MEMORY.md', summary: 'Loaded', durationMs: 12, status: 'completed' },
        { id: 's6', tool: 'grep', target: 'src/', summary: 'Indexing', durationMs: 24, status: 'running' },
        { id: 's7', tool: 'bash', target: 'cargo check', summary: 'Verify', durationMs: 0, status: 'pending' },
      ],
    },
  ];
  const running = computeTaskProgress(streamingMsg, true);
  assert.ok(running);
  assert.equal(running.status, 'running');
  assert.equal(running.percentage, 50);

  // Denied step halts progress
  const deniedMsg: Message[] = [
    { id: 'u3', sender: 'user', timestamp: '10:06:00', content: 'Edit' },
    {
      id: 'a4',
      sender: 'agent',
      agentRole: 'Tinker',
      timestamp: '10:06:01',
      content: 'Stopped.',
      toolSteps: [
        { id: 's8', tool: 'read', target: 'a.ts', summary: 'Read', durationMs: 10, status: 'completed' },
        { id: 's9', tool: 'edit', target: 'a.ts', summary: 'Denied', durationMs: 10, status: 'denied' },
      ],
    },
  ];
  const denied = computeTaskProgress(deniedMsg, false);
  assert.ok(denied);
  assert.equal(denied.status, 'denied');
  assert.equal(denied.percentage, 50);
});

test('formatElapsedMs and pickCompanionReactionQuip format deterministic UI states', () => {
  assert.equal(formatElapsedMs(420), '0.4s');
  assert.equal(formatElapsedMs(65_000), '1m 05s');
  assert.equal(pickCompanionReactionQuip(null, 'Meowstro'), null);
  assert.ok(pickCompanionReactionQuip('celebrate', 'Meowstro')?.includes('Purr-fect'));
  assert.ok(pickCompanionReactionQuip('petted', 'Oyen')?.includes('Oyen'));
});
