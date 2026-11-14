import type {
  AgentRole,
  ApprovalRequest,
  AuditEntry,
  AuditOutcome,
  Message,
  ToolStep,
} from '../types/cockpit';

export interface PathValidationResult {
  valid: boolean;
  normalized?: string;
  error?: string;
}

const SENSITIVE_PREFIXES = [
  '/etc',
  '/root',
  '/var/run',
  '/proc',
  '/sys',
  '/dev',
  '/boot',
  '/usr/sbin',
  '/sbin',
];

const SENSITIVE_FRAGMENTS = ['.ssh', '.gnupg', '.aws', '.kube', 'id_rsa', 'id_ed25519'];

export function validateSandboxPath(rawPath: string): PathValidationResult {
  const trimmed = rawPath.trim();
  if (!trimmed) {
    return { valid: false, error: 'Path cannot be empty.' };
  }
  if (trimmed.includes('\0')) {
    return { valid: false, error: 'Null bytes are prohibited in sandbox paths.' };
  }
  if (trimmed.includes('..')) {
    return {
      valid: false,
      error: 'Path traversal segments ("..") are blocked by Landlock policy.',
    };
  }

  // Normalize duplicate slashes and trailing slashes (except root '/')
  let normalized = trimmed.replace(/\/+/g, '/');
  if (normalized.length > 1 && normalized.endsWith('/')) {
    normalized = normalized.slice(0, -1);
  }

  if (normalized === '/' || normalized === '~') {
    return {
      valid: false,
      error: 'Root or entire home directory cannot be allowlisted directly.',
    };
  }

  const lower = normalized.toLowerCase();
  for (const prefix of SENSITIVE_PREFIXES) {
    if (lower === prefix || lower.startsWith(`${prefix}/`)) {
      return {
        valid: false,
        error: `Access to sensitive system path "${prefix}" is denied by fail-closed policy.`,
      };
    }
  }

  for (const frag of SENSITIVE_FRAGMENTS) {
    if (lower.includes(frag)) {
      return {
        valid: false,
        error: `Credential directory or key pattern "${frag}" cannot be exposed to agents.`,
      };
    }
  }

  if (!normalized.startsWith('/') && !normalized.startsWith('~/') && !normalized.startsWith('./')) {
    return {
      valid: false,
      error: 'Path must be absolute (/workspace/...), home-relative (~/...), or workspace-relative (./...).',
    };
  }

  return { valid: true, normalized };
}

export interface CronParseResult {
  valid: boolean;
  expression: string;
  summary: string;
  nextRun: string;
  error?: string;
}

function formatNextRun(minutesFromNow: number): string {
  const d = new Date(Date.now() + minutesFromNow * 60_000);
  const hh = String(d.getHours()).padStart(2, '0');
  const mm = String(d.getMinutes()).padStart(2, '0');
  return `Today at ${hh}:${mm}`;
}

export function parseNaturalLanguageCron(input: string): CronParseResult {
  const raw = input.trim();
  if (!raw) {
    return {
      valid: false,
      expression: '',
      summary: '',
      nextRun: '',
      error: 'Enter a schedule in plain English or 5-field cron syntax.',
    };
  }

  // Standard 5-field cron check
  const parts = raw.split(/\s+/);
  if (parts.length === 5 && /^[\d*,/-]+$/.test(parts[0]) && /^[\d*,/-]+$/.test(parts[1])) {
    return {
      valid: true,
      expression: raw,
      summary: `Custom cron expression (${raw})`,
      nextRun: formatNextRun(15),
    };
  }

  const lower = raw.toLowerCase();

  // "every N minutes" / "tiap N menit"
  const minMatch = lower.match(/(?:every|tiap)\s+(\d+)\s*(?:min|minute|minutes|menit)/);
  if (minMatch) {
    const mins = Math.max(1, Math.min(59, Number(minMatch[1])));
    return {
      valid: true,
      expression: `*/${mins} * * * *`,
      summary: `Runs every ${mins} minute${mins === 1 ? '' : 's'}`,
      nextRun: formatNextRun(mins),
    };
  }

  // "hourly" / "every hour" / "tiap jam"
  if (/\b(hourly|every hour|tiap jam)\b/.test(lower)) {
    return {
      valid: true,
      expression: '0 * * * *',
      summary: 'Runs at the start of every hour',
      nextRun: formatNextRun(60),
    };
  }

  // "weekdays at HH:MM" or "weekdays at 9am"
  const weekdayMatch = lower.match(
    /(?:weekdays|mon-fri|senin-jumat).*?(?:at|pukul|jam)?\s*(\d{1,2})(?::(\d{2}))?\s*(am|pm)?/
  );
  if (weekdayMatch) {
    let hour = Number(weekdayMatch[1]);
    const minute = Number(weekdayMatch[2] ?? '0');
    if (weekdayMatch[3] === 'pm' && hour < 12) hour += 12;
    if (weekdayMatch[3] === 'am' && hour === 12) hour = 0;
    if (hour > 23 || minute > 59) {
      return { valid: false, expression: '', summary: '', nextRun: '', error: 'Invalid time in schedule.' };
    }
    const hh = String(hour).padStart(2, '0');
    const mm = String(minute).padStart(2, '0');
    return {
      valid: true,
      expression: `${minute} ${hour} * * 1-5`,
      summary: `Runs Monday–Friday at ${hh}:${mm}`,
      nextRun: `Next weekday at ${hh}:${mm}`,
    };
  }

  // "daily at HH:MM" / "every day at 09:30" / "setiap hari jam 9"
  const dailyMatch = lower.match(
    /(?:daily|every day|setiap hari|tiap hari).*?(?:at|pukul|jam)?\s*(\d{1,2})(?::(\d{2}))?\s*(am|pm)?/
  );
  if (dailyMatch) {
    let hour = Number(dailyMatch[1]);
    const minute = Number(dailyMatch[2] ?? '0');
    if (dailyMatch[3] === 'pm' && hour < 12) hour += 12;
    if (dailyMatch[3] === 'am' && hour === 12) hour = 0;
    if (hour > 23 || minute > 59) {
      return { valid: false, expression: '', summary: '', nextRun: '', error: 'Invalid time in schedule.' };
    }
    const hh = String(hour).padStart(2, '0');
    const mm = String(minute).padStart(2, '0');
    return {
      valid: true,
      expression: `${minute} ${hour} * * *`,
      summary: `Runs daily at ${hh}:${mm}`,
      nextRun: `Tomorrow at ${hh}:${mm}`,
    };
  }

  // "every N hours"
  const hourMatch = lower.match(/(?:every|tiap)\s+(\d+)\s*(?:hr|hour|hours|jam)/);
  if (hourMatch) {
    const hrs = Math.max(1, Math.min(23, Number(hourMatch[1])));
    return {
      valid: true,
      expression: `0 */${hrs} * * *`,
      summary: `Runs every ${hrs} hour${hrs === 1 ? '' : 's'}`,
      nextRun: formatNextRun(hrs * 60),
    };
  }

  return {
    valid: false,
    expression: '',
    summary: '',
    nextRun: '',
    error: 'Try phrasing like "every 15 minutes", "hourly", "daily at 09:30", or "weekdays at 8am".',
  };
}

export interface SlashCommandDef {
  command: string;
  usage: string;
  description: string;
}

export const SLASH_COMMANDS: SlashCommandDef[] = [
  { command: '/plan', usage: '/plan <goal>', description: 'Ask Meowstro to synthesize an architectural plan first' },
  { command: '/model', usage: '/model <model-id>', description: 'Switch active session model immediately' },
  { command: '/preset', usage: '/preset <tabby|carrier|oyen>', description: 'Switch execution autonomy preset' },
  { command: '/skill', usage: '/skill <name>', description: 'Invoke or inspect a procedural skill' },
  { command: '/clear', usage: '/clear', description: 'Clear active session timeline and start fresh' },
];

export function matchSlashCommands(input: string): SlashCommandDef[] {
  const trimmed = input.trimStart();
  if (!trimmed.startsWith('/')) return [];
  const token = trimmed.split(/\s+/)[0].toLowerCase();
  return SLASH_COMMANDS.filter((item) => item.command.startsWith(token));
}

export interface ContextMetrics {
  usedTokens: number;
  limitTokens: number;
  percentUsed: number;
  status: 'nominal' | 'elevated' | 'critical';
}

export function computeContextMetrics(messages: Message[], limitTokens: number): ContextMetrics {
  const safeLimit = Math.max(1024, limitTokens);
  let charCount = 4200; // Base system + SOUL.md + MEMORY.md prompt footprint
  for (const msg of messages) {
    charCount += msg.content.length;
    if (msg.toolSteps) {
      for (const step of msg.toolSteps) {
        charCount += step.target.length + step.summary.length + 60;
      }
    }
    if (msg.diff) {
      for (const h of msg.diff.hunks) {
        charCount += h.content.length + 12;
      }
    }
  }
  const usedTokens = Math.min(safeLimit, Math.round(charCount / 3.6));
  const percentUsed = Math.min(100, Math.round((usedTokens / safeLimit) * 100));
  const status = percentUsed >= 85 ? 'critical' : percentUsed >= 65 ? 'elevated' : 'nominal';
  return { usedTokens, limitTokens: safeLimit, percentUsed, status };
}

export function filterAuditEntries(
  entries: AuditEntry[],
  outcomeFilter: 'All' | AuditOutcome,
  query: string
): AuditEntry[] {
  const q = query.trim().toLowerCase();
  return entries.filter((entry) => {
    if (outcomeFilter !== 'All' && entry.outcome !== outcomeFilter) {
      return false;
    }
    if (!q) return true;
    return (
      entry.actor.toLowerCase().includes(q) ||
      entry.operation.toLowerCase().includes(q) ||
      entry.target.toLowerCase().includes(q) ||
      entry.detail.toLowerCase().includes(q)
    );
  });
}

export function resolveApprovalState(
  approval: ApprovalRequest,
  decision: 'approved' | 'denied' | 'expired'
): ApprovalRequest {
  if (approval.status !== 'pending') {
    return approval;
  }
  return {
    ...approval,
    status: decision,
    resolvedAt: new Date().toLocaleTimeString('en-GB', { hour12: false }),
  };
}

export interface TaskProgressStep {
  id: string;
  label: string;
  tool: ToolStep['tool'] | 'synth';
  status: ToolStep['status'];
}

export interface TaskProgressState {
  status: 'running' | 'awaiting-approval' | 'completed' | 'denied';
  percentage: number;
  completedCount: number;
  totalSteps: number;
  currentStepLabel: string;
  activeActor: AgentRole;
  steps: TaskProgressStep[];
}

export function computeTaskProgress(
  messages: Message[],
  isStreaming: boolean
): TaskProgressState | null {
  if (messages.length === 0 && !isStreaming) {
    return null;
  }

  let lastUserIdx = -1;
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    if (messages[i].sender === 'user') {
      lastUserIdx = i;
      break;
    }
  }

  const turnMessages = messages
    .slice(lastUserIdx + 1)
    .filter((m) => m.sender === 'agent');

  if (turnMessages.length === 0 && !isStreaming) {
    return null;
  }

  const collectedSteps: TaskProgressStep[] = [];
  let activeActor: AgentRole = 'Meowstro';
  let pendingApprovalAction: string | null = null;
  let hasDeniedApproval = false;

  for (const msg of turnMessages) {
    if (msg.agentRole) {
      activeActor = msg.agentRole;
    }
    if (msg.approval?.status === 'pending') {
      pendingApprovalAction = msg.approval.action;
    }
    if (msg.approval?.status === 'denied' || msg.approval?.status === 'expired') {
      hasDeniedApproval = true;
    }
    if (msg.toolSteps) {
      for (const step of msg.toolSteps) {
        collectedSteps.push({
          id: step.id,
          label: `${step.tool}: ${step.target}`,
          tool: step.tool,
          status: step.status,
        });
      }
    }
  }

  if (collectedSteps.length === 0) {
    if (!isStreaming) return null;
    return {
      status: 'running',
      percentage: 45,
      completedCount: 0,
      totalSteps: 1,
      currentStepLabel: 'Synthesizing response stream',
      activeActor,
      steps: [
        {
          id: 'synth-active',
          label: 'Synthesizing response',
          tool: 'synth',
          status: 'running',
        },
      ],
    };
  }

  const hasRunning = collectedSteps.some((s) => s.status === 'running');
  const hasPending = collectedSteps.some((s) => s.status === 'pending');
  const hasDeniedStep = collectedSteps.some((s) => s.status === 'denied') || hasDeniedApproval;

  // Hide banner for completed single-step historical chats unless streaming/pending
  if (collectedSteps.length < 2 && !isStreaming && !hasRunning && !hasPending && !pendingApprovalAction) {
    return null;
  }

  let weighted = 0;
  let completedCount = 0;
  for (const step of collectedSteps) {
    if (step.status === 'completed') {
      weighted += 1;
      completedCount += 1;
    } else if (step.status === 'running') {
      weighted += 0.5;
    }
  }

  const rawPct = Math.round((weighted / collectedSteps.length) * 100);
  const percentage = Math.min(100, Math.max(0, rawPct));

  let status: TaskProgressState['status'];
  if (hasDeniedStep) {
    status = 'denied';
  } else if (isStreaming || hasRunning) {
    status = 'running';
  } else if (hasPending || pendingApprovalAction) {
    status = 'awaiting-approval';
  } else {
    status = 'completed';
  }

  const runningStep = collectedSteps.find((s) => s.status === 'running');
  const pendingStep = collectedSteps.find((s) => s.status === 'pending');
  const deniedStep = collectedSteps.find((s) => s.status === 'denied');

  let currentStepLabel: string;
  if (status === 'denied') {
    currentStepLabel = deniedStep
      ? `Halted at ${deniedStep.label}`
      : 'Task halted by fail-closed policy';
  } else if (status === 'awaiting-approval') {
    currentStepLabel = pendingApprovalAction
      ? `Awaiting approval · ${pendingApprovalAction}`
      : `Awaiting approval · ${pendingStep?.label ?? 'staged operation'}`;
  } else if (status === 'running') {
    currentStepLabel = runningStep
      ? `Running ${runningStep.label}`
      : 'Synthesizing multi-step output';
  } else {
    currentStepLabel = `All ${collectedSteps.length} task steps completed`;
  }

  return {
    status,
    percentage: status === 'completed' ? 100 : percentage,
    completedCount,
    totalSteps: collectedSteps.length,
    currentStepLabel,
    activeActor,
    steps: collectedSteps,
  };
}

export function formatElapsedMs(ms: number): string {
  const safeMs = Math.max(0, ms);
  if (safeMs < 60_000) {
    return `${(safeMs / 1000).toFixed(1)}s`;
  }
  const totalSec = Math.floor(safeMs / 1000);
  const mins = Math.floor(totalSec / 60);
  const secs = String(totalSec % 60).padStart(2, '0');
  return `${mins}m ${secs}s`;
}

export function pickCompanionReactionQuip(
  reaction: 'celebrate' | 'petted' | 'handoff' | null,
  companionName: string
): string | null {
  if (!reaction) return null;
  if (reaction === 'celebrate') {
    return `${companionName}: Purr-fect! All steps verified & complete.`;
  }
  if (reaction === 'petted') {
    return `${companionName}: Purrrr~ Keeping the sandbox spotless!`;
  }
  return `${companionName}: Handing task baton to Clowder squad.`;
}
