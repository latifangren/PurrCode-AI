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

- [x] **2.1 File & Execution Tools (`crates/purrcode-tools/`)**
  - `read`: Baca file dengan window limit/offset (teruji).
  - `write`: Tulis file atomik (teruji).
  - `edit`: Replace exact hash-anchored match + preview diff (teruji).
  - `bash`: Eksekusi terminal lokal dengan timeout (teruji).
  - `grep`, `find`: Pencarian file berbasis regex & walkdir (teruji).
- [x] **2.2 Approval Gate & Audit (`crates/purrcode-approval/`)**
  - Evaluasi deny > ask > allow.
  - Tiering persetujuan (`ApprovalTier::Read`, `Write`, `Exec`).
- [x] **2.3 MCP Client Bridge (`crates/purrcode-tools/src/mcp/`)**
  - Spawner stdio process JSON-RPC 2.0.
  - Handshake protocol `initialize` & `notifications/initialized`.
  - Pendaftaran tool MCP dinamis ke `ToolRegistry` (`McpDynamicTool`).
- [x] **2.4 Agent Loop Interaktif Non-TUI (`crates/purrcode-core/`)**
  - Multi-turn loop: prompt -> model -> tool call -> tool result -> model -> final answer.
  - **Milestone 2:** Terverifikasi sukses menjalankan tool `read`, `bash`, dan `find` secara live.

---

## Sprint 3: TUI Terminal Interaktif (Ratatui)
> **Tujuan:** Pengalaman terminal penuh dengan card tool-call, preview diff, dan prompt interaktif `purr>`.

- [x] **3.1 TUI Layout & Event Bus (`crates/purrcode-channel/src/tui/`)**
  - Setup terminal crossterm + Ratatui (differential rendering).
  - Layout: Header status bar, transcript scroll view, footer prompt `purr>`.
- [x] **3.2 Widgets Khusus & Card Tools**
  - `tool_card`: Tampilan card status pemanggilan tool (`[running]`, `[done]`, `[failed]`).
  - `render_transcript`: Render user input, thinking reasoning block, dan tool call output.
  - Palet warna Zinc 900 + Aksen Oranye Tabby + Hijau Emerald.
- [x] **3.3 Interupsi & Keybindings**
  - `Ctrl-C` / `Esc` untuk membatalkan/keluar.
  - Navigasi kursor teks kiri/kanan, backspace, scroll up/down.
  - Command `/quit` dan `/exit` untuk keluar rapi dan restore terminal.
  - **Milestone 3:** `purrcode-ai` (tanpa argumen) membuka TUI interaktif responsif.

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
