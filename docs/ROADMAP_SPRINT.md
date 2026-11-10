# PurrCode AI — Panduan Eksekusi Vibe Coding (ROADMAP_SPRINT.md)

Dokumen ini adalah **checklist eksekusi bertahap** untuk implementasi kode.  
Tiap sprint memiliki deliverable konkret yang bisa langsung diuji (`cargo run`).

---

## Sprint 1: One-Shot LLM CLI (`purrcode-ai -p "<prompt>"`)
> **Tujuan:** Binary bisa membaca config custom endpoint (Magpie/Ollama/OpenRouter), streaming respon ke terminal, dan selesai.

- [ ] **1.1 Config Loader (`crates/purrcode-config/`)**
  - Baca file `~/.purrcode/purrcode.jsonc` (atau `purrcode.jsonc` lokal).
  - Parse mapping `providers` dan model endpoint.
  - Test: Unit test parsing `purrcode.jsonc.example`.
- [ ] **1.2 OpenAI-Compatible Provider (`crates/purrcode-provider/`)**
  - Implementasi `reqwest` SSE client untuk custom endpoint `POST /v1/chat/completions`.
  - Dukungan streaming text chunk & token usage.
  - Dukungan parameter `reasoning_effort` (low/medium/high) bila endpoint mendukung.
  - Test: Panggilan langsung ke `http://127.0.0.1:3425/v1` atau Ollama lokal.
- [ ] **1.3 Simple CLI One-Shot (`crates/purrcode-cli/`)**
  - Tambahkan perintah `purrcode-ai -p "<prompt>"` dan cetak streaming jawaban ke stdout.
  - Guard: Cetak error bersih bila belum ada model yang dikonfigurasi.
  - **Milestone 1:** `cargo run -p purrcode-cli -- -p "halo PurrCode"` berhasil menjawab secara streaming.

---

## Sprint 2: 12 Tools Inti & Jembatan MCP
> **Tujuan:** Agent loop bisa memanggil tool, mengeksekusi aksi file/bash, dan memanggil server MCP.

- [ ] **2.1 File & Execution Tools (`crates/purrcode-tools/`)**
  - `read`: Baca file dengan window limit/offset.
  - `write`: Tulis file atomik.
  - `edit`: Replace exact hash-anchored match + preview diff.
  - `bash`: Eksekusi terminal lokal (timeout, auto-background, sandboxed to workspace).
  - `grep`, `glob`, `find`: Pencarian file berbasis ripgrep / walkdir.
- [ ] **2.2 Approval Gate & Audit (`crates/purrcode-approval/`)**
  - Evaluasi deny > ask > allow.
  - Pencatatan semua pemanggilan tool ke `~/.purrcode/audit.jsonl` (scrubbing secret).
- [ ] **2.3 MCP Client Bridge (`crates/purrcode-tools/src/mcp/`)**
  - Parser `~/.purrcode/mcp.json`.
  - Spawner stdio process JSON-RPC 2.0 (Playwright, CodeGraph, dll.).
  - Pendaftaran tool MCP otomatis ke `ToolRegistry`.
- [ ] **2.4 Agent Loop Interaktif Non-TUI (`crates/purrcode-core/`)**
  - Loop: prompt -> model -> tool call -> tool result -> model -> final answer.
  - **Milestone 2:** `purrcode-ai -p "baca file Cargo.toml lalu sebutkan versinya"` berhasil membaca file dan menjawab.

---

## Sprint 3: TUI Terminal Interaktif (Ratatui)
> **Tujuan:** Pengalaman terminal penuh dengan card tool-call, preview diff, dan prompt interaktif `purr>`.

- [ ] **3.1 TUI Layout & Event Bus (`crates/purrcode-channel/src/tui/`)**
  - Setup terminal crossterm + Ratatui.
  - Layout: Header status bar, transcript scroll view, footer prompt `purr>`.
- [ ] **3.2 Widgets Khusus**
  - `tool_card`: Tampilan card untuk pemanggilan tool yang sedang berjalan (running/done/failed).
  - `edit_preview`: Diff visual sebelum menulis file dengan tombol `[a]` Terapkan, `[t]` Tolak.
  - `ask_picker`: Pilihan opsi terstruktur saat model memanggil tool `ask`.
- [ ] **3.3 Interupsi & Keybindings**
  - `Ctrl-C` / `Esc` untuk membatalkan run tanpa keluar dari program.
  - `Tab` autocomplete slash commands (`/preset`, `/model`, `/clear`, `/quit`).
  - **Milestone 3:** `purrcode-ai` membuka TUI interaktif yang responsif dan nyaman digunakan.

---

## Sprint 4: Meowstro & Subagents (The Clowder)
> **Tujuan:** Orchestrator dan subagent anti-hilang-konteks.

- [ ] **4.1 Meowstro Engine (`crates/purrcode-orchestrator/`)**
  - Enforce tool set Meowstro (read-only + delegation: `read`, `grep`, `glob`, `find`, `todo`, `ask`, `task`).
  - Tolak pemanggilan `write`/`edit`/`bash` di level runtime.
- [ ] **4.2 Continuation Registry & Handoff Files**
  - Pembuatan ID stabil `purr_<id>`.
  - Penulisan otomatis `brief.md` dan `result.md` di direktori sesi.
  - Subagent Tinker (Fixer) & Whisker (Explorer).
- [ ] **4.3 Memori Persisten Markdown (`crates/purrcode-memory/`)**
  - Pemuatan `MEMORY.md`, `USER.md`, `SOUL.md`.
  - Auto-compaction non-destruktif.
  - **Milestone 4:** Meowstro mendelegasikan tugas kompleks ke Tinker, Tinker mengedit kode dan verifikasi test, lalu melaporkan kembali ke Meowstro tanpa Meowstro kehilangan konteks rencana.
