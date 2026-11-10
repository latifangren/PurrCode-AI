# PurrCode AI — Panduan Eksekusi Vibe Coding (ROADMAP_SPRINT.md)

Dokumen ini adalah **checklist eksekusi bertahap** untuk implementasi kode.  
Tiap sprint memiliki deliverable konkret yang bisa langsung diuji (`cargo run`).

---

## Sprint 1: One-Shot LLM CLI (`purrcode-ai -p "<prompt>"`)
> **Tujuan:** Binary bisa membaca config custom endpoint (Magpie/Ollama/OpenRouter), streaming respon ke terminal, dan selesai.

- [x] **1.1 Config Loader (`crates/purrcode-config/`)**
  - Baca file `~/.purrcode/purrcode.jsonc` (atau `purrcode.jsonc` lokal) dengan stripper JSONC komentar & trailing comma (teruji).
  - Parse mapping `providers` dan model endpoint custom (teruji).
  - Test: Unit test parsing `purrcode.jsonc.example` (lulus).
- [x] **1.2 OpenAI-Compatible Provider (`crates/purrcode-provider/`)**
  - Implementasi `reqwest` SSE client untuk custom endpoint `POST /v1/chat/completions` (teruji).
  - Dukungan streaming text chunk, reasoning delta (`thought`/`reasoning_content`), & token usage (teruji).
  - Dukungan parameter `reasoning_effort` (low/medium/high) (teruji).
  - Test: Panggilan langsung terverifikasi ke Magpie `http://127.0.0.1:3425/v1` (lulus).
- [x] **1.3 Simple CLI One-Shot (`crates/purrcode-cli/`)**
  - Perintah `purrcode-ai -p "<prompt>"` mencetak streaming jawaban ke stdout (teruji).
  - Flag `--json` mengalirkan NDJSON stream (teruji).
  - Guard: Menolak jalan bila model belum dikonfigurasi dengan pesan panduan bersih (teruji).
  - **Milestone 1:** Terverifikasi sukses streaming respon dari model live.

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

- [x] **4.1 Meowstro Engine (`crates/purrcode-orchestrator/`)**
  - Enforce tool set Meowstro di level runtime: read-only + delegasi `task` (`read`, `grep`, `find`, `task`) (teruji).
  - Meowstro tidak memiliki tool `write`, `edit`, atau `bash` (teruji).
- [x] **4.2 Continuation Registry & Handoff Files**
  - Pembuatan ID stabil `purr_<id>` (teruji).
  - Penulisan otomatis `brief.md` dan `result.md` di direktori sesi fisik `subagents/<id>/` (teruji langsung di disk).
  - Subagent Tinker (Fixer dengan full tools) & Whisker (Explorer read-only) (teruji).
  - Dukungan delegasi ke `tuxeshadow` (Shadow Auditor) (teruji).
- [x] **4.3 Memori Persisten Markdown (`crates/purrcode-memory/`)**
  - Pemuatan konteks memori hierarkis (`USER.md`, `MEMORY.md`, `.purrcode/MEMORY.md`) ke prompt sistem (teruji).
  - Trait `MemoryBackend` dengan dukungan `remember` (append) dan `recall` (teruji).
  - **Milestone 4:** Terverifikasi sukses menjalankan Meowstro yang mendelegasikan tugas ke Tinker, mencatat handoff ke disk, dan menghasilkan laporan sintesis.
