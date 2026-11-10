# PurrCode AI 🐾

Agen AI modular dalam **Rust** (**GPL-3.0**, single static binary `purrcode-ai`) — orchestrator utama (**Meowstro**) + tim subagent spesialis (**The Clowder**) yang tidak kehilangan konteks, memori persisten Markdown, sistem plugin, TUI terminal berbasis Ratatui, serta dukungan MCP (`mcp.json` / `purrcode.jsonc`).

Fokus v0.1: **Terminal CLI / TUI**. Fase berikutnya: Telegram Gateway (v0.2), Otonom & OS Sandbox (v0.3), dan WebUI khusus (v0.4).

## Struktur Repositori
- `crates/` — 12 crate Rust workspace dengan rantai dependensi satu arah (`purrcode-api` → `purrcode-config` → `purrcode-core` → `purrcode-provider` → `purrcode-tools` / `purrcode-memory` / `purrcode-approval` / `purrcode-plugin` → `purrcode-orchestrator` / `purrcode-channel` / `purrcode-service` → `purrcode-cli`)
- `presets/` — Mode presets (`preset/v1`): `carrier` (strict), `tabby` (default supervised), `oyen` (god mode)
- `model-presets/` — Model presets (`model-preset/v1`): `free`, `local`, `premium`, `custom-endpoint`
- `agents/` — Definisi 7 agen The Clowder (`meowstro`, `whisker`, `sphinx`, `calico`, `tinker`, `tuxeshadow`, `clowder`)
- `skills/` — 8 skill bawaan v0.1 (`plan`, `review`, `git`, `debug`, `scaffold`, `frontend-design`, `webapp-testing`, `doctor`) dengan kontrak routing di frontmatter
- `docs/` — [Analisis Referensi](docs/00-analysis-references.md), [PRD](docs/PRD.md), [ARCHITECTURE](docs/ARCHITECTURE.md), [DESIGN](docs/DESIGN.md), [DECISIONS](docs/DECISIONS.md), [STRUCTURE](docs/STRUCTURE.md), [SUBAGENTS](docs/SUBAGENTS.md), [MCP](docs/MCP.md), [LSP](docs/LSP.md), [DEPENDENCIES](docs/DEPENDENCIES.md), [ROADMAP_SPRINT](docs/ROADMAP_SPRINT.md)
- `references/` — Analisis 13 proyek referensi open-source (clone mentah di-gitignore)

## Menjalankan & Menguji
```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p purrcode-cli -- onboard
cargo run -p purrcode-cli -- doctor
```
