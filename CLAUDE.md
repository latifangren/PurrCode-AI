# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commonly Used Commands

### Build & Check
- Build workspace: `cargo build --workspace`
- Build release binary: `cargo build --release`
- Format check: `cargo fmt --check`
- Apply formatting: `cargo fmt`
- Lint workspace (zero warnings policy): `cargo clippy --workspace --all-targets -- -D warnings`

### Testing
- Run all tests across workspace: `cargo test --workspace`
- Run all tests in a specific crate: `cargo test -p purrcode-tools`
- Run a single test by name: `cargo test -p purrcode-tools test_manage_skill_lifecycle`
- Run tests with output printed: `cargo test -p purrcode-tools -- --nocapture`

### Running the Application
- Run interactive TUI: `cargo run -p purrcode-cli`
- Run one-shot non-interactive prompt: `cargo run -p purrcode-cli -- -p "your prompt"`
- Run onboarding setup wizard: `cargo run -p purrcode-cli -- onboard`
- Run environment and system diagnostics: `cargo run -p purrcode-cli -- doctor`
- Specify an explicit workspace root: `cargo run -p purrcode-cli -- -w <path>`

---

## High-Level Architecture & Rules

### 1. Language & License
- Language: **Rust 1.80+**, compiles into a single binary (`purrcode-ai`; musl static is the distribution target — current builds are dynamic GNU, see `docs/AUDIT_STATUS.md`), licensed under **GNU GPL-3.0**.
- **English Code Rule:** All identifiers, comments, error messages, and log strings MUST be written in English. Documentation may be in Indonesian or English.

### 2. Workspace Crate Architecture (Strict One-Way Dependency Chain)
The repository is split into 12 modular crates with a one-way dependency chain:
1. `purrcode-api`: Base contracts, error types (`PurrError`, `Result`), tool traits (`Tool`, `ToolContext`), and event enums (`AgentEvent`).
2. `purrcode-config`: Configuration loader, zero-hardcoded dynamic provider/model resolution, mode presets (`presets/*.jsonc`), and model presets (`model-presets/*.jsonc`).
3. `purrcode-core`: Agent execution engine (`AgentLoop`), steering queue, cancellation flags, and session JSONL logging.
4. `purrcode-provider`: Model provider integrations (OpenAI-compatible streaming, Ollama, vLLM, reasoning tokens `reasoning_content`/`thought`, and fallback routing).
5. Domain Crates:
   - `purrcode-tools`: Core 13 tools (fs: `read`, `write`, `edit`; exec: `bash`; search: `grep`, `glob`, `find`; coord: `ask`, `todo`; web: `webfetch`, `websearch`; skill: `read_skill`, `manage_skill`).
   - `purrcode-memory`: Markdown-based persistent memory (`MEMORY.md`, `USER.md`, `SOUL.md`), zero external DB requirement.
   - `purrcode-approval`: Fail-closed policy gate (`ApprovalGate`), policy tiers, regex path rules, and approval timeout handlers.
   - `purrcode-plugin`: Plugin manifest loader (`plugin.yaml`), lifecycle hook dispatcher (`pre_tool_use`, `post_tool_use`), and default-deny secret scrubbing. `network`/`filesystem` capabilities are declarations for the v0.3 OS sandbox, not enforced in v0.1 (process plugins run with full user rights).
6. Upper Layer Crates:
   - `purrcode-orchestrator`: The Clowder multi-agent coordinator (`MeowstroOrchestrator`), subagent delegator (`TaskTool`), continuation registry (`SubagentRegistry`), and shadow auditor (`Tuxeshadow`).
   - `purrcode-channel`: TUI interface (Ratatui differential renderer) and multi-channel abstraction.
   - `purrcode-service`: System daemon integration (systemd unit and OpenRC init service generators).
7. Top-Level Binary:
   - `purrcode-cli`: CLI argument parsing (Clap), onboard wizard, interactive TUI lifecycle, and one-shot execution.

### 3. The Clowder Multi-Agent Invariants
- **Meowstro Never Writes Code:** Meowstro is strictly a read-only orchestrator (`read`, `grep`, `glob`, `find`, `todo`, `ask`, `task`, `read_skill`). Meowstro **never** has `write`, `edit`, `bash`, or `manage_skill` in his toolset. Implementation is always delegated to subagents.
- **Tinker is the Fixer/Implementer:** Tinker receives the full implementation toolset (including `write`, `edit`, `bash`, and `manage_skill`).
- **Subagents Never Lose Context:** Every delegated subagent receives a persistent continuation ID (`purr_<id>`) and handoff files (`brief.md` + `result.md`). Subagents resume the existing ID rather than respawning. Depth is strictly 1 (no nested subagents in v0.1).
- **Tuxeshadow Shadow Auditor:** Evaluates plan JSON and diff previews before execution; mandatory for cron jobs and replanning.

### 4. Safety & Operational Constraints
- **Fail-Closed Supervised Approvals:** Supervised permissions follow `deny > ask > allow`. Deny always wins, and approval timeout defaults to deny.
- **Path Confinement:** Filesystem tools strictly canonicalize and confine operations to `workspace_root` to block directory traversal (`..`).
- **SSRF Mitigation:** Network tools (`webfetch`, `websearch`) block private, loopback, link-local, multicast, Carrier-Grade NAT (`100.64.0.0/10`), and benchmarking ranges, reinforced by async DNS resolution to defeat DNS-rebinding attacks.
- **Coding Ergonomics:**
  - `edit`: Hash-anchored exact replacement with fallback fuzzy CRLF/whitespace matching.
  - `grep`: Context lines (`-C`) formatted with standard diff syntax (`:` for matches, `-` for context).
  - `bash`: Smart output truncation (60% head + 40% tail) to preserve LLM context budget.
- **Self-Improving Procedural Skills:** Skills live in `skills/` (embedded), `.purrcode/skills/` (project), or `~/.purrcode/skills/` (global). The `manage_skill` tool allows dynamic creation, fuzzy patching with unified diffs, and deletion with automatic fallback restoration.
- **No Hardcoded Defaults:** There is no hardcoded fallback model. If no model is configured, the application refuses to run silently and guides the user to configure an endpoint.
- **No Production Panics:** Never use `unwrap()` or `expect()` on production paths; wrap errors using `purrcode_api::error::Result` and context-rich `PurrError`.
- **Git Push Protocol:** Never push to a remote repository without explicit user approval per action.
