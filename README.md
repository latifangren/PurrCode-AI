# PurrCode AI 🐾

Agent AI modular — orchestrator utama + subagent yang tidak hilang konteks,
memory persisten, plugin seperti DeepSeek Harness, TUI terminal seperti oh-my-pi,
dan channel Telegram seperti Hermes Agent.

Fokus v0.1: **Terminal CLI / TUI dulu**. Telegram fase 2.
Bahasa target: **Rust (keputusan user 2026-11-11)** — diputuskan di `docs/ARCHITECTURE.md`
Bab 9; Go dicatat sebagai alternatif yang dievaluasi. Lisensi: **GPL-3.0**.

## Struktur
- `docs/` — [analisis referensi](docs/00-analysis-references.md), [PRD](docs/PRD.md), [ARCHITECTURE](docs/ARCHITECTURE.md), [DESIGN](docs/DESIGN.md)
- `references/` — clone shallow (read-only, ~2 GB, gitignored) repo open-source yang ditiru/dikombinasikan
- `src/` — kode PurrCode (scaffold menyusul setelah docs disetujui user)

## Referensi (di-clone ke references/)
- openai/codex — sandbox + Rust CLI + approval policy
- can1357/oh-my-pi (+ earendil-works/pi) — TUI, 32 tools, subagent, memory Hindsight
- sst/opencode — server TS (v2) + plugin in-process (host oh-my-opencode-slim)
- alvinunreal/oh-my-opencode-slim — Pantheon: Orchestrator/Explorer/Oracle/Council/Librarian/Designer/Fixer
- deepseek-ai/deepseek-harness — "Everything is a Plugin" (Cordis)
- NousResearch/hermes-agent — gateway Telegram/Discord, cron, self-improving skills, memory
- tinyhumansai/openhuman — Rust core, Telegram, Memory Tree SQLite
- sipeed/picoclaw (Go) + zeroclaw-labs/zeroclaw (Rust) — single-binary ringan, Telegram, supervised autonomy
- Muse (App + Code) — harness perilaku, MSP, skill routing (dari `Muse.zip` user, `references/muse/`)

Lisensi tiap referensi tetap milik upstream — lihat `references/<nama>/ANALYSIS.md`
dan file LICENSE di dalam clone masing-masing. PurrCode TIDAK meng-copy kode
mentah-mentah; yang diambil pola/arsitektur, dengan atribusi.
