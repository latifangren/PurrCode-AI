export type CockpitView =
  | 'chat'
  | 'routing'
  | 'providers'
  | 'sandbox'
  | 'memory'
  | 'mcp'
  | 'cron'
  | 'appearance';

export type PresetMode = 'tabby' | 'carrier' | 'oyen';

export type AgentRole =
  | 'Meowstro'
  | 'Tinker'
  | 'Whisker'
  | 'Sphinx'
  | 'Calico'
  | 'Tuxeshadow';

export type ToolStatus = 'completed' | 'running' | 'pending' | 'denied';

export interface ToolStep {
  id: string;
  tool: 'read' | 'grep' | 'edit' | 'bash' | 'mcp';
  target: string;
  summary: string;
  durationMs: number;
  status: ToolStatus;
}

export interface DiffLine {
  lineOld: number | null;
  lineNew: number | null;
  type: 'context' | 'remove' | 'add';
  content: string;
}

export interface UnifiedDiff {
  filePath: string;
  language: string;
  hunks: DiffLine[];
  status: 'pending' | 'accepted' | 'rejected';
}

export interface ApprovalRequest {
  id: string;
  action: string;
  target: string;
  riskLevel: 'high' | 'medium';
  timeoutSeconds: number;
  status: 'pending' | 'approved' | 'denied' | 'expired';
  resolvedAt?: string;
}

export interface Message {
  id: string;
  sender: 'user' | 'agent';
  agentRole?: AgentRole;
  agentTitle?: string;
  modelUsed?: string;
  thinkingDurationMs?: number;
  timestamp: string;
  content: string;
  pinnedFiles?: string[];
  toolSteps?: ToolStep[];
  diff?: UnifiedDiff;
  approval?: ApprovalRequest;
  isStreaming?: boolean;
}

export interface Session {
  id: string;
  title: string;
  branch: string;
  preset: PresetMode;
  model: string;
  updatedAt: string;
  messages: Message[];
}

export interface SubagentRouting {
  role: AgentRole;
  responsibility: string;
  primaryModel: string;
  fallbackModel: string;
  temperature: number;
  maxTokens: number;
  status: 'idle' | 'active';
}

export interface ProviderConfig {
  id: string;
  name: string;
  protocol: 'gemini' | 'anthropic' | 'openai' | 'deepseek' | 'ollama' | 'custom';
  baseUrl: string;
  apiKey?: string;
  defaultModel: string;
  models: string[];
  contextWindow: number;
  enabled: boolean;
  lastLatencyMs: number | null;
  lastTestedAt: string | null;
}

export interface SandboxPathRule {
  id: string;
  path: string;
  mode: 'ro' | 'rw';
  addedAt: string;
}

export type AuditOutcome = 'Allowed' | 'Denied' | 'Violation';

export interface AuditEntry {
  id: string;
  timestamp: string;
  actor: AgentRole | 'System';
  operation: string;
  target: string;
  outcome: AuditOutcome;
  detail: string;
}

export interface SandboxState {
  landlockAbi: string;
  enforced: boolean;
  outboundNetworkEnabled: boolean;
  blockedCidrs: string[];
  paths: SandboxPathRule[];
  auditLog: AuditEntry[];
}

export interface MemoryFile {
  name: 'MEMORY.md' | 'USER.md' | 'SOUL.md';
  description: string;
  content: string;
  updatedAt: string;
}

export interface SkillEntry {
  id: string;
  name: string;
  scope: 'bundled' | 'user' | 'project';
  description: string;
  trigger: string;
  enabled: boolean;
  diffPreview?: UnifiedDiff;
}

export interface McpToolSchema {
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
}

export interface McpServer {
  id: string;
  name: string;
  transport: 'stdio' | 'sse';
  endpoint: string;
  status: 'connected' | 'disconnected' | 'error';
  latencyMs: number | null;
  tools: McpToolSchema[];
}

export interface CronJob {
  id: string;
  naturalPrompt: string;
  expression: string;
  summary: string;
  nextRun: string;
  targetAgent: AgentRole;
  enabled: boolean;
}

export interface ThemeTokens {
  canvas: string;
  surfaceSoft: string;
  surfaceCard: string;
  surfaceStrong: string;
  ink: string;
  body: string;
  bodyStrong: string;
  muted: string;
  mutedSoft: string;
  hairline: string;
  hairlineSoft: string;
  primary: string;
  primaryActive: string;
  primaryDisabled: string;
  onPrimary: string;
  codeBg: string;
  codeSoft: string;
  codeElevated: string;
  codeBorder: string;
  codeText: string;
  codeMuted: string;
  accentTeal: string;
  accentAmber: string;
  success: string;
  warning: string;
  error: string;
}

export interface ThemePreset {
  id: string;
  name: string;
  description: string;
  isDark: boolean;
  tokens: ThemeTokens;
}

export type CompanionPattern = 'tuxedo' | 'tabby' | 'calico' | 'siamese' | 'solid';
export type CompanionAccessory = 'none' | 'glasses' | 'headphones' | 'scarf';
export type CompanionVisibility = 'full' | 'compact' | 'hidden';
export type CompanionMood = 'idle' | 'thinking' | 'approval' | 'excited';

export interface CompanionConfig {
  presetId: string;
  name: string;
  tagline: string;
  pattern: CompanionPattern;
  furPrimary: string;
  furSecondary: string;
  eyeColor: string;
  accessory: CompanionAccessory;
  visibility: CompanionVisibility;
}

export type CompanionReaction = 'celebrate' | 'petted' | 'handoff' | null;

export type ToastTone = 'success' | 'info' | 'warning' | 'error';

export interface ToastItem {
  id: string;
  tone: ToastTone;
  title: string;
  detail?: string;
}

export interface CockpitPreferences {
  activeThemeId: string;
  customThemeTokens: ThemeTokens;
  companion: CompanionConfig;
}

export interface CockpitState {
  schemaVersion?: number;
  projectName: string;
  projectPath: string;
  activeSessionId: string;
  sessions: Session[];
  routing: SubagentRouting[];
  providers: ProviderConfig[];
  sandbox: SandboxState;
  memoryFiles: MemoryFile[];
  skills: SkillEntry[];
  mcpServers: McpServer[];
  cronJobs: CronJob[];
  preferences: CockpitPreferences;
}
