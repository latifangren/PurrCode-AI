import { DEFAULT_PREFERENCES } from '../themes/themeRegistry';
import type { CockpitState } from '../types/cockpit';

export const SEED_COCKPIT_STATE: CockpitState = {
  projectName: 'PurrCode-AI',
  projectPath: '~/Project/PurrCode-AI',
  activeSessionId: 'session-12',
  sessions: [
    {
      id: 'session-12',
      title: 'New Chat',
      branch: 'main',
      preset: 'tabby',
      model: 'antigravity/gemini-3.8-flash-tiered',
      updatedAt: 'Just now',
      messages: [],
    },
  ],
  routing: [
    {
      role: 'Meowstro',
      responsibility: 'Lead Orchestrator & Task Planner',
      primaryModel: 'antigravity/gemini-3.8-flash-tiered',
      fallbackModel: 'deepseek-v3',
      temperature: 0.2,
      maxTokens: 8192,
      status: 'active',
    },
    {
      role: 'Tinker',
      responsibility: 'Code Implementer & Surgical Patch Fixer',
      primaryModel: 'antigravity/gemini-3.8-flash-tiered',
      fallbackModel: 'qwen-2.5-coder-32b',
      temperature: 0.1,
      maxTokens: 16384,
      status: 'active',
    },
    {
      role: 'Whisker',
      responsibility: 'Codebase Explorer & Fast Grep/AST Recon',
      primaryModel: 'antigravity/gemini-3.8-flash-tiered',
      fallbackModel: 'qwen-2.5-coder-32b',
      temperature: 0.0,
      maxTokens: 4096,
      status: 'idle',
    },
    {
      role: 'Sphinx',
      responsibility: 'Architecture Oracle & Security Reviewer',
      primaryModel: 'antigravity/gemini-3.8-flash-tiered',
      fallbackModel: 'deepseek-r1',
      temperature: 0.15,
      maxTokens: 8192,
      status: 'idle',
    },
    {
      role: 'Calico',
      responsibility: 'Editorial UI Designer & Component Architect',
      primaryModel: 'antigravity/gemini-3.8-flash-tiered',
      fallbackModel: 'claude-sonnet-4',
      temperature: 0.4,
      maxTokens: 8192,
      status: 'idle',
    },
    {
      role: 'Tuxeshadow',
      responsibility: 'Background Verification & Test Harness Runner',
      primaryModel: 'antigravity/gemini-3.8-flash-tiered',
      fallbackModel: 'ollama/llama3.3',
      temperature: 0.0,
      maxTokens: 4096,
      status: 'idle',
    },
  ],
  providers: [
    {
      id: 'prov-magpie',
      name: 'Local Magpie / OpenAI-Compatible',
      protocol: 'openai',
      baseUrl: 'http://127.0.0.1:3425/v1',
      defaultModel: 'antigravity/gemini-3.8-flash-tiered',
      models: [
        'antigravity/gemini-3.8-flash-tiered',
      ],
      contextWindow: 1048576,
      enabled: true,
      lastLatencyMs: 12,
      lastTestedAt: 'Just now',
    },
    {
      id: 'prov-custom',
      name: 'Custom Gateway Endpoint',
      protocol: 'custom',
      baseUrl: 'http://localhost:8000/v1',
      defaultModel: 'antigravity/gemini-3.8-flash-tiered',
      models: ['antigravity/gemini-3.8-flash-tiered'],
      contextWindow: 1048576,
      enabled: true,
      lastLatencyMs: null,
      lastTestedAt: null,
    },
  ],
  sandbox: {
    landlockAbi: 'ABI v3',
    enforced: true,
    outboundNetworkEnabled: false,
    blockedCidrs: ['10.0.0.0/8', '172.16.0.0/12', '192.168.0.0/16', '127.0.0.0/8', '169.254.0.0/16'],
    paths: [
      { id: 'sp-1', path: '~/Project/PurrCode-AI', mode: 'rw', addedAt: 'Session start' },
      { id: 'sp-2', path: '/usr/local/cargo/registry', mode: 'ro', addedAt: 'System default' },
      { id: 'sp-3', path: '/usr/include', mode: 'ro', addedAt: 'System default' },
      { id: 'sp-4', path: '~/.purrcode/skills', mode: 'ro', addedAt: 'System default' },
    ],
    auditLog: [],
  },
  memoryFiles: [
    {
      name: 'MEMORY.md',
      description: 'Persistent architectural facts, verified patterns, and project conventions.',
      updatedAt: 'Today, 14:15',
      content: `# PurrCode-AI Project Memory

## Architectural Invariants
- **ADR-011 Modularity**: Keep every source module focused and under 800 lines.
- **Fail-Closed Security**: Landlock ABI v3 denies writes outside explicitly allowlisted workspace paths.
- **Supervised Mode (Tabby)**: All destructive filesystem mutations and shell side-effects require explicit operator confirmation with a 30s auto-deny timeout.

## Recent Decisions
- Adopted Warm Cream + Coral + Dark Product Chrome design language for the developer cockpit.
- Consolidated provider and sandbox parity endpoints behind authenticated REST + SSE routes.`,
    },
    {
      name: 'USER.md',
      description: 'Operator preferences, coding style, and workflow defaults.',
      updatedAt: 'Yesterday, 20:40',
      content: `# Operator Profile

- **Primary Languages**: Rust, TypeScript, SQL
- **Preferred Review Style**: Unified diffs with explicit file line numbers before applying patches.
- **Autonomy Default**: Tabby (supervised approval for edits and commands).
- **Communication**: Concise engineering summaries; surface security invariants directly.`,
    },
    {
      name: 'SOUL.md',
      description: 'Core persona, reasoning discipline, and multi-agent Clowder coordination rules.',
      updatedAt: '2026-11-12',
      content: `# PurrCode Clowder Charter

1. **Meowstro (Orchestrator)** decomposes requests and verifies invariants before delegating.
2. **Tinker (Fixer)** produces minimal, surgical diffs and never refactors unrelated code.
3. **Whisker (Explorer)** inspects the codebase read-only before assumptions are formed.
4. **Never fabricate tool outputs** — report exact paths, test assertions, and security boundaries.`,
    },
  ],
  skills: [
    {
      id: 'sk-plan',
      name: 'plan',
      scope: 'bundled',
      description: 'Pre-flight multi-step implementation plan with explicit rollback checkpoints.',
      trigger: 'Any multi-file edit or structural refactoring prompt',
      enabled: true,
    },
    {
      id: 'sk-review',
      name: 'review',
      scope: 'bundled',
      description: 'Unified diff and AST invariant review against ADR-011 and memory rules.',
      trigger: 'Pre-merge checks or user review request',
      enabled: true,
    },
    {
      id: 'sk-git',
      name: 'git',
      scope: 'bundled',
      description: 'Branch inspection, stash operations, and worktree isolation.',
      trigger: 'Git operations with safety verification',
      enabled: true,
    },
    {
      id: 'sk-debug',
      name: 'debug',
      scope: 'bundled',
      description: 'Failure root cause analysis and reproduction synthesis.',
      trigger: 'Failing tests or runtime panics',
      enabled: true,
    },
    {
      id: 'sk-scaffold',
      name: 'scaffold',
      scope: 'bundled',
      description: 'Deterministic crate and module generation respecting ADR-011 limits.',
      trigger: 'New feature scaffolding prompts',
      enabled: true,
    },
    {
      id: 'sk-frontend',
      name: 'frontend-design',
      scope: 'bundled',
      description: 'Component polish, token enforcement, and responsive layout audit.',
      trigger: 'UI and styling changes',
      enabled: true,
    },
    {
      id: 'sk-doctor',
      name: 'doctor',
      scope: 'bundled',
      description: 'Workspace sanity, toolchain readiness, and Landlock ABI check.',
      trigger: 'Onboarding or diagnostic command',
      enabled: true,
    },
  ],
  mcpServers: [
    {
      id: 'mcp-fs',
      name: 'Local Filesystem & Git Inspector',
      transport: 'stdio',
      endpoint: 'purrcode-mcp-fs',
      status: 'connected',
      latencyMs: 14,
      tools: [
        {
          name: 'fs_list_untracked',
          description: 'Enumerate untracked files without altering git index',
          inputSchema: {
            type: 'object',
            properties: {
              include_untracked: { type: 'boolean' },
            },
          },
        },
      ],
    },
  ],
  cronJobs: [
    {
      id: 'cron-1',
      naturalPrompt: 'every 15 minutes',
      expression: '*/15 * * * *',
      summary: 'Runs every 15 minutes',
      nextRun: 'Today at 14:30',
      targetAgent: 'Tuxeshadow',
      enabled: true,
    },
    {
      id: 'cron-2',
      naturalPrompt: 'weekdays at 9:00am',
      expression: '0 9 * * 1-5',
      summary: 'Runs Monday–Friday at 09:00',
      nextRun: 'Next weekday at 09:00',
      targetAgent: 'Sphinx',
      enabled: true,
    },
  ],
  preferences: DEFAULT_PREFERENCES,
};
