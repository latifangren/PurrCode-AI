# AGENTS.md — Working Instructions for PurrCode AI (repo)

> File ini adalah **instruksi kerja untuk agen (dan manusia) yang mengerjakan
> repo ini**. File ini BUKAN bagian dari produk PurrCode itu sendiri — tidak
> di-ship ke dalam binary, tidak dibaca oleh PurrCode saat berjalan. Isinya
> aturan cara kerja di repo ini.
>
> Aturan bahasa (keputusan user): **kode ditulis dalam bahasa Inggris**
> (identifier, komentar, pesan error, log) — **dokumentasi boleh bahasa
> Inggris atau Indonesia** (dokumen repo ini memakai Indonesia).

## 1. What PurrCode is

PurrCode AI is a modular AI agent, written in **Rust**, licensed **GPL-3.0**,
single static musl binary. Main agent (orchestrator) + subagents that do not
lose context, persistent memory, plugins like DeepSeek Harness, terminal TUI
like oh-my-pi, Telegram like Hermes Agent. Focus v0.1: **terminal CLI / TUI
first**; Telegram v0.2; WebUI v0.4.

One sentence: a lightweight, modular Muse/Hermes in the user's own coding style.

## 2. Read first — before touching anything

1. `docs/PRD.md` — what to build, in what order (§4), and what is decided (§7).
2. `docs/ARCHITECTURE.md` — how it is built (components §2, agents §3,
   memory §4, plugins §5, security §7, directories §8, Rust mapping §9).
3. `docs/DESIGN.md` — TUI/Telegram/WebUI design + ADRs (§9–§13).
4. `docs/00-analysis-references.md` — what was taken from which open-source
   reference, and what must NOT be copied (licenses, §1).

Do not re-derive decisions that are already recorded. If a decision in these
docs contradicts your instinct, the docs win until the user changes them.

## 3. The agent team (The Clowder)

| Agent | Role | Tool set (enforced at runtime, not by prompt) |
|---|---|---|
| Meowstro | Orchestrator, the user's representative | `read`, `grep`, `glob`, `find`, `todo`, `ask`, `task`, `read_skill` — **never `write`, `edit`, `bash`, `manage_skill`** |
| Whisker | Explorer (absorbs Librarian) | read-only (`read`, `grep`, `glob`, `find`) + `webfetch`, `websearch` |
| Sphinx | Oracle | read-only (`read`, `grep`, `glob`, `find`) + `ask` |
| Calico | Designer (UI/TUI) | `read`, `write`, `edit`, `grep`, `glob`, `find` (no `bash`) |
| Tinker | Fixer — the only full implementer | all v0.1 implementation tools (including `manage_skill`, no `task`) |
| Tuxeshadow | Shadow Auditor — independent plan/risk critic | read-only (`read`, `grep`, `glob`, `find`) + `ask` (no `task`, no `write`/`edit`/`bash`) |
| Clowder | Council, 2–3 seats, different models | read-only (`read`, `grep`, `glob`, `find`) |

Rules:

- **Meowstro never writes code.** He understands the codebase (map,
  architecture, key files), plans, delegates, verifies — implementation always
  goes to Tinker. This protects the main context: it holds map + summaries +
  decisions only. No exception for "faster to do it myself".
- **Subagents never lose context.** Every delegation gets a stable
  continuation ID (`purr_<id>`); resume the same ID, never respawn.
  Handoff files `subagents/<id>/brief.md` + `result.md` + registry
  `subagents/<id>.json`. Child returns a structured summary (changed
  file:line, failures, unverified items). No nesting in v0.1 (depth max 1).
- A child may only **narrow** its parent's policy — never escalate to a
  looser mode than the parent.

## 4. Presets — never hardcode a mode

Two preset kinds, separate namespaces:

- **Mode preset** (`presets/*.jsonc`, schema `preset/v1`): behavior =
  approval × guardrail × sandbox. Built-ins: `carrier` (Strict),
  `tabby` (Default — this is the default), `oyen` (God). Selected via
  `--preset` / config / per-project override. Adding a mode = adding a file.
- **Model preset** (`model-presets/*.jsonc`, schema `model-preset/v1`):
  inference settings per subagent. Built-ins: `free` (default),
  `local`, `premium`, `custom-endpoint`, + user custom files (name = filename).

`yolo` is about **approval** only; `god` is about **guardrails** only — two
separate decisions, two separate flags. `oyen` is never `yolo` automatically.
`oyen` never offered in onboarding; explicit per-session opt-in + confirm.

Layered guardrails: agent mode → provider ToS (each provider enforces its own;
the harness cannot bypass it). A provider policy refusal is surfaced as-is and
**the fallback chain is never used to evade it** — fallback runs only for
technical errors (timeout, 5xx, quota). Audit log can never be disabled, in
any mode. Cron jobs never run `oyen`/`yolo`.

## 5. Memory

- Default is **Markdown only**: `MEMORY.md` / `USER.md` / `SOUL.md`
  (+ `.purrcode/MEMORY.md` per project) + session JSONL. No database required.
- Memory backends (FTS5, vector, external) are **plugins** behind the
  versioned `MemoryBackend` trait — never hardcode one into core.
- **No breaking change:** the MD format and session JSONL format are stable
  contracts. External backends must dual-write to MD. Trait changes get a new
  version + adapter; v1 is never changed after release.
- When you learn a durable fact, preference, decision, or outcome about the
  user or the project, record it in the right memory file before replying.
  Routine progress and tool output do not belong in memory.

## 6. How to work (verify-first)

1. **Read before you write.** Inspect the real tree (git status/diff, tests)
   before changing it. Never verify by manual comparison or a stale copy.
2. **Work to completion.** Take a task from intent to a finished, verified
   result. A claim without evidence is not done.
3. **Tests before claims.** Every feature ships with tests (unit + PRD §4.5
   scenario). Record test results in the PR, don't claim them.
4. **Small, proper, multi-file structure.** No single mixed file; one
   responsibility per file/crate; one-way crate chain (enforced by CI).
5. **Rust idiomatic:** `cargo fmt` + `cargo clippy -D warnings`; errors wrapped
   with context (`anyhow`/`thiserror`); no `panic!`/`unwrap()`/`expect()` on
   production paths; no moralizing or "educational only" comments.
6. **Free-first.** No paid path as the only way. BYO keys + Ollama/local first.
   Never recommend a paid subscription as a requirement.
7. **Git workflow:** `main` stays stable; features on branches; PR with
   description + test evidence. **Push only on the user's explicit per-action
   approval** — a standing approval never carries over, and this rule holds
   even for docs-only commits.

## 7. Safety & secrets

- **Supervised fail-closed by default.** Ask before destructive actions;
  deny wins over ask over allow; approval timeout → deny.
- **Secrets:** tokens live in `~/.purrcode/credentials` (0600), never in
  config, chat, logs, or memory. Never send credentials to an LLM without a
  `[FILTERED]` filter. Never ask the user to paste a password/API key in chat
  — use the secure entry flow.
- **Plugins:** manifest declares `capabilities` (network, fs, secrets);
  default deny. A plugin never receives secrets unless the capability is
  declared AND the user approved it at install.
- **References:** patterns only, never copied code. GPL references: patterns
  OK. Non-commercial (Sustainable Use) / proprietary: patterns only, never
  code. Anthropic document skills (`docx`/`pdf`/`pptx`/`xlsx`) are
  source-available and forbid derivatives — write document skills from
  scratch if needed. The leaked Claude Code source map (DMCA) must never
  be used.
- **Targets:** ≤ 25 MB binary, ≤ 50 MB idle RAM, TUI startup < 1s on PC,
  < 3s on a 2 GB STB. Offline-first where possible (local + Ollama without
  internet).

## 8. Skills & Self-Improvement

Bundled v0.1 (8, written from scratch, patterns from Muse Code's 28 skills
and `anthropics/skills`' 19): `plan`, `review`, `git`, `debug`, `scaffold`,
`frontend-design`, `webapp-testing`, `doctor`. Format: `SKILL.md` +
frontmatter (`name`, `description`) — the description is a **routing
contract** (what loads / must-not-load the skill). Only descriptions load at
startup; full bodies load on demand via `read_skill`. Discovery precedence:
`.purrcode/skills/` (project) → `~/.purrcode/skills/` (user global) → bundled.
Do not bundle all 28 — each description costs ~100 tokens of startup context.

**Procedural Memory & Self-Improvement:**
Tinker can dynamically synthesize, patch (with unified diff preview), or delete
skills at runtime using the `manage_skill` tool without recompilation. Deleting
an overridden bundled skill safely restores the embedded fallback.

## 9. Decisions live here

- Current decisions: `docs/PRD.md` §7, `docs/ARCHITECTURE.md` §11,
  `docs/DESIGN.md` ADRs (§9–§13), `docs/AUDIT_STATUS.md`.
- When a decision changes: update the docs + record an ADR
  (`docs/DECISIONS.md` once implementation starts). Never leave a stale
  decision standing next to new code.
- Decisions settled (2026-11-11):
  1. Binary name is `purrcode-ai`.
  2. Repo is public (`latifangren/purrcode-ai`).
  3. No hardcoded default model — user must explicitly configure model / custom
     endpoints (custom baseUrl, apiKey, models, routing groups ala opencode & omp
     config). Agent refuses to run silently without configured model and prompts
     user to configure or run onboard.
  4. Procedural skills can be dynamically synthesized/managed at runtime by
     Tinker via `manage_skill` (inspired by Hermes Agent & Muse AI).
  5. Fail-closed SSRF protection with async DNS resolution and Carrier-Grade
     NAT/benchmarking IP blocks across all network tools (`webfetch`, `websearch`).
  6. Coding ergonomics parity with Claude Code & Codex (fuzzy whitespace/CRLF
     `edit` matching, `grep` context lines `-C`, `bash` smart output truncation).
  7. Explicit workspace root override flag `-w, --workspace <path>` supported
     across CLI, TUI, and subcommands.
