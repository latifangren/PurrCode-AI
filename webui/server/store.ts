import fs from 'node:fs';
import path from 'node:path';
import { SEED_COCKPIT_STATE } from '../src/data/seedCockpitState';
import { DEFAULT_PREFERENCES } from '../src/themes/themeRegistry';
import type { AgentRole, AuditOutcome, CockpitState } from '../src/types/cockpit';

export const CURRENT_SCHEMA_VERSION = 2;

export function nowTime(): string {
  return new Date().toLocaleTimeString('en-GB', { hour12: false });
}

export function migrateCockpitState(raw: unknown): {
  state: CockpitState;
  migrated: boolean;
} {
  const base = structuredClone(SEED_COCKPIT_STATE);
  if (!raw || typeof raw !== 'object') {
    return {
      state: { ...base, schemaVersion: CURRENT_SCHEMA_VERSION },
      migrated: true,
    };
  }

  const parsed = raw as Partial<CockpitState>;
  const incomingVersion = typeof parsed.schemaVersion === 'number' ? parsed.schemaVersion : 1;
  const migrated = incomingVersion < CURRENT_SCHEMA_VERSION;

  const merged: CockpitState = {
    ...base,
    ...parsed,
    schemaVersion: CURRENT_SCHEMA_VERSION,
    sessions:
      Array.isArray(parsed.sessions) && parsed.sessions.length > 0
        ? parsed.sessions
        : base.sessions,
    routing:
      Array.isArray(parsed.routing) && parsed.routing.length > 0
        ? parsed.routing
        : base.routing,
    providers:
      Array.isArray(parsed.providers) && parsed.providers.length > 0
        ? parsed.providers
        : base.providers,
    sandbox: parsed.sandbox
      ? {
          ...base.sandbox,
          ...parsed.sandbox,
          paths: Array.isArray(parsed.sandbox.paths)
            ? parsed.sandbox.paths
            : base.sandbox.paths,
          auditLog: Array.isArray(parsed.sandbox.auditLog)
            ? parsed.sandbox.auditLog
            : base.sandbox.auditLog,
        }
      : base.sandbox,
    memoryFiles:
      Array.isArray(parsed.memoryFiles) && parsed.memoryFiles.length > 0
        ? parsed.memoryFiles
        : base.memoryFiles,
    skills:
      Array.isArray(parsed.skills) && parsed.skills.length > 0
        ? parsed.skills
        : base.skills,
    mcpServers: Array.isArray(parsed.mcpServers) ? parsed.mcpServers : base.mcpServers,
    cronJobs: Array.isArray(parsed.cronJobs) ? parsed.cronJobs : base.cronJobs,
    preferences: {
      ...DEFAULT_PREFERENCES,
      ...(parsed.preferences ?? {}),
      customThemeTokens: {
        ...DEFAULT_PREFERENCES.customThemeTokens,
        ...(parsed.preferences?.customThemeTokens ?? {}),
      },
      companion: {
        ...DEFAULT_PREFERENCES.companion,
        ...(parsed.preferences?.companion ?? {}),
      },
    },
  };

  if (!merged.sessions.some((s) => s.id === merged.activeSessionId)) {
    merged.activeSessionId = merged.sessions[0].id;
  }

  return { state: merged, migrated };
}

export class AtomicStateStore {
  private readonly filePath: string;
  private readonly startedAtMs: number;
  public state: CockpitState;

  constructor(filePath = path.resolve(process.cwd(), '.purrcode-state.json')) {
    this.filePath = filePath;
    this.startedAtMs = Date.now();
    this.state = this.load();
  }

  private load(): CockpitState {
    try {
      if (fs.existsSync(this.filePath)) {
        const rawText = fs.readFileSync(this.filePath, 'utf-8');
        const parsed = JSON.parse(rawText);
        const { state, migrated } = migrateCockpitState(parsed);
        if (migrated) {
          // Create a safe pre-migration backup snapshot before updating schema version
          try {
            fs.writeFileSync(`${this.filePath}.bak`, rawText, 'utf-8');
          } catch {
            // Backup failure should not block cockpit boot
          }
          this.writeAtomic(state);
        }
        return state;
      }
    } catch (err) {
      console.warn('Could not read persisted state file, using seed state:', err);
    }

    return {
      ...structuredClone(SEED_COCKPIT_STATE),
      schemaVersion: CURRENT_SCHEMA_VERSION,
    };
  }

  private writeAtomic(data: CockpitState): void {
    const dir = path.dirname(this.filePath);
    if (!fs.existsSync(dir)) {
      fs.mkdirSync(dir, { recursive: true });
    }
    const tmpPath = `${this.filePath}.${process.pid}.tmp`;
    const serialized = JSON.stringify(data, null, 2);
    fs.writeFileSync(tmpPath, serialized, 'utf-8');
    fs.renameSync(tmpPath, this.filePath);
  }

  public persist(): void {
    try {
      this.state.schemaVersion = CURRENT_SCHEMA_VERSION;
      this.writeAtomic(this.state);
    } catch (err) {
      console.warn('Failed to persist atomic state file:', err);
    }
  }

  public appendAudit(
    actor: AgentRole | 'System',
    operation: string,
    target: string,
    outcome: AuditOutcome,
    detail: string
  ): void {
    this.state.sandbox.auditLog.unshift({
      id: `aud-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
      timestamp: nowTime(),
      actor,
      operation,
      target,
      outcome,
      detail,
    });
    if (this.state.sandbox.auditLog.length > 50) {
      this.state.sandbox.auditLog.length = 50;
    }
  }

  public getHealthSummary() {
    return {
      status: 'ok',
      schemaVersion: this.state.schemaVersion ?? CURRENT_SCHEMA_VERSION,
      uptimeSeconds: Math.floor((Date.now() - this.startedAtMs) / 1000),
      sessionsCount: this.state.sessions.length,
      persistenceMode: 'atomic-json',
      stateFileExists: fs.existsSync(this.filePath),
    };
  }
}
