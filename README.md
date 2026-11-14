# PurrCode AI 🐾

Agen AI modular dalam **Rust** (**GPL-3.0**, single binary `purrcode-ai`) — orchestrator utama (**Meowstro**) + tim subagent spesialis (**The Clowder**) yang mempertahankan konteks lewat continuation ID (`purr_<id>`), harness eksekusi rekayasa perangkat lunak (read-before-edit guard, parallel read batching, anti-loop reminder, auto-compaction terstruktur), memori persisten Markdown (+ auto-load `AGENTS.md`/`CLAUDE.md`), TUI terminal Ratatui, Telegram Gateway, OS sandbox (Landlock), serta WebUI Cockpit terpadu.

## Fitur Utama
- **Multi-Channel & Cockpit Terpadu:** Terminal TUI (`purrcode-cli`), one-shot (`-p`), Telegram Gateway (`gateway`), dan WebUI Cockpit (`web`, React 19 + Axum REST/SSE di port tunggal).
- **Engineering Harness (`purrcode-core`):** Shared `CORE_HARNESS_CONTRACT` untuk mode orchestrator, direct mode, dan seluruh subagent; eksekusi paralel untuk batch read-only; `SessionFileGuard` (read-before-edit guard); `FailureLoopTracker` anti-loop `<system-reminder>`; serta structured compaction + auto-compact berbasis batas konteks.
- **The Clowder Multi-Agent:** `Meowstro` (hybrid workflow orchestrator), `Tinker` (implementer), `Whisker` (read-only explorer/librarian), `Sphinx` (architecture oracle), `Calico` (UI/TUI designer), `Tuxeshadow` (read-only shadow auditor), dan `Clowder` (multi-model council).
- **Memori & Instruksi Proyek:** `SOUL.md`, `USER.md`, `MEMORY.md`, `.purrcode/MEMORY.md`, serta pemuatan otomatis `.purrcode/AGENTS.md`, `AGENTS.md`, dan `CLAUDE.md`.
- **Keamanan Fail-Closed:** Policy tier (`deny > ask > allow`), workspace path confinement, proteksi SSRF + DNS rebinding pada `webfetch`/`websearch`, dan OS Landlock sandbox (`carrier`/`tabby`/`oyen`).

## Struktur Repositori
- `crates/` — 12 crate Rust workspace dengan rantai dependensi satu arah (`purrcode-api` → `purrcode-config` → `purrcode-core` → `purrcode-provider` → `purrcode-tools` / `purrcode-memory` / `purrcode-approval` / `purrcode-plugin` → `purrcode-orchestrator` / `purrcode-channel` / `purrcode-service` → `purrcode-cli`)
- `webui/` — Frontend WebUI Cockpit (React 19, TypeScript, Tailwind CSS v4, Vite)
- `presets/` — Mode presets (`preset/v1`): `carrier` (strict), `tabby` (default supervised), `oyen` (god mode)
- `model-presets/` — Model presets (`model-preset/v1`): `free`, `local`, `premium`, `custom-endpoint`
- `agents/` — Definisi 7 agen The Clowder (`meowstro`, `whisker`, `sphinx`, `calico`, `tinker`, `tuxeshadow`, `clowder`) dengan override opsional di `.purrcode/agents/<role>.md`
- `skills/` — 8 skill bawaan (`plan`, `review`, `git`, `debug`, `scaffold`, `frontend-design`, `webapp-testing`, `doctor`) dengan kontrak routing on-demand di frontmatter
- `docs/` — [Analisis Referensi](docs/00-analysis-references.md), [PRD](docs/PRD.md), [ARCHITECTURE](docs/ARCHITECTURE.md), [DESIGN](docs/DESIGN.md), [DECISIONS](docs/DECISIONS.md), [STRUCTURE](docs/STRUCTURE.md), [SUBAGENTS](docs/SUBAGENTS.md), [MCP](docs/MCP.md), [LSP](docs/LSP.md), [DEPENDENCIES](docs/DEPENDENCIES.md), [ROADMAP_SPRINT](docs/ROADMAP_SPRINT.md)
- `references/` — Analisis proyek referensi open-source (clone mentah di-gitignore)

## Menjalankan & Menguji
```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

cargo run -p purrcode-cli -- onboard
cargo run -p purrcode-cli -- doctor
cargo run -p purrcode-cli -- -p "inspect workspace"
cargo run -p purrcode-cli -- web --port 8080
```
