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

---

## Sprint 5: Sisa Tool Inti & Skill Engine (Agentskills)
> **Tujuan:** Menyelesaikan 12 tool inti v0.1 dan sistem pemuatan skill berbasis SKILL.md.

- [x] **5.1 Tool Coordination (`crates/purrcode-tools/src/coordination/`)**
  - `todo`: Pelacak progres rencana kerja (`[ ]`, `[▶]`, `[✓]`, `[-]`) (teruji).
  - `ask`: Dialog pertanyaan terstruktur ke user dengan pilihan opsi saat ada ambiguitas (teruji).
- [x] **5.2 Tool Web (`crates/purrcode-tools/src/web/`)**
  - `webfetch`: Mengambil dan mengekstrak teks halaman web/API via reqwest dengan tag stripping (teruji).
  - `websearch`: Pencarian web tanpa browser berat via DuckDuckGo HTML extractor (teruji).
- [x] **5.3 Skill Engine (`crates/purrcode-tools/src/skill/` & `skills/`)**
  - Discovery hirarkis `skills/` bawaan, `~/.purrcode/skills/`, dan `.purrcode/skills/` (teruji).
  - Pemuatan hemat token: deskripsi frontmatter di awal, isi lengkap via tool `read_skill` (teruji).
  - 8 Skill bawaan v0.1 selesai dibuat: `plan`, `review`, `git`, `debug`, `scaffold`, `frontend-design`, `webapp-testing`, `doctor`.
- [x] **Milestone 5:** Terverifikasi live eksekusi `todo`, `ask`, dan pemuatan on-demand `read_skill git`.

---

## Sprint 6: Integrasi TUI Live Stream & Approval Card Interaktif
> **Tujuan:** Menghubungkan AgentLoop langsung ke TUI Ratatui dengan card preview diff interaktif.

- [x] **6.1 TUI Live Event Stream**
  - Antrian event async `TuiEvent` terhubung ke TUI thread-safe (teruji).
  - Transkrip merender streaming delta, reasoning thinking, dan tool cards secara realtime.
- [x] **6.2 Interactive Approval Card Widget**
  - Widget Approval Card pop-up dengan diff preview dan border aksen oranye (teruji).
  - Tombol interaktif: `[a]` Setuju/Apply, `[t]` Tolak/Deny, `[e]` Edit (teruji).
- [x] **6.3 Onboarding Wizard Interaktif (`purrcode-ai onboard`)**
  - Panduan interaktif setup endpoint (Local Proxy Magpie, Ollama, Cloud API), API key, model, dan preset yang langsung menulis `purrcode.jsonc` (teruji).

---

## Sprint 7: Konfigurasi Berlapis, Preset Engine, dan Provider Router
> **Tujuan:** Menutup celah konfigurasi hierarkis, gate konteks 64K, loader `preset/v1` & `model-preset/v1`, serta fallback router.

- [x] **7.1 Hierarchical Config Merge & 64K Context Gate (`crates/purrcode-config/`)**
  - Merge berjenjang system -> `~/.purrcode/purrcode.jsonc` -> `purrcode.jsonc` -> `.purrcode/purrcode.jsonc` + `mcp.json` (teruji).
  - Validasi `MIN_CONTEXT_WINDOW = 65536` menolak model < 64K (teruji).
- [x] **7.2 Mode Preset (`preset/v1`) & Model Preset (`model-preset/v1`) Loaders**
  - Discovery dan parsing dinamis `presets/*.jsonc` dan `model-presets/*.jsonc` dengan dukungan `extends` satu parent (teruji).
- [x] **7.3 Provider Default Base URLs, Fallback Router & `ProviderRefusal` (`crates/purrcode-provider/`)**
  - Default base URL untuk 11 provider bawaan + Ollama/LM Studio (teruji).
  - `FallbackRouter` dengan cooldown error teknis dan penghentian rantai saat `PurrError::ProviderRefusal` (teruji).

---

## Sprint 8: Session Store, Stateful Subagent Resume, dan Penegakan Toolset 7 Agen
> **Tujuan:** Menyelesaikan kontrak anti-hilang-konteks subagent, penyimpanan sesi JSONL, dan toolset The Clowder.

- [x] **8.1 Session Store & Non-Destructive Compaction (`crates/purrcode-core/src/session.rs`)**
  - Append-only `sessions/<id>.jsonl`, `fork_session`, `export_session` (JSONL/HTML), dan `compact_messages` (teruji).
  - Dry-run diff preview diteruskan ke `ApprovalRequester` sebelum eksekusi tool tulis/eksekusi (teruji).
- [x] **8.2 Stateful Subagent Resume & Disk Registry (`crates/purrcode-orchestrator/`)**
  - Persistensi `subagents/<id>.json`, `subagents/<id>/history.json`, `brief.md`, `result.md`, dan `workgraph.json` (teruji).
  - Skenario PRD §4.5: delegasi berantai & resume menggunakan continuation ID `purr_<id>` yang sama dengan riwayat utuh (teruji).
- [x] **8.3 Penegakan Toolset 7 Agen, Clowder Council, & Hash-Anchored Edit**
  - Penegakan toolset `meowstro`, `whisker`, `sphinx`, `calico`, `tinker`, `tuxeshadow`, dan 2-seat paralel `clowder` (teruji).
  - Hash-anchoring FNV-1a (`expected_hash` / `content_hash`) dan workspace confinement pada `read`, `write`, `edit`, `bash`, `find`, `glob`, `grep` (teruji).

---

## Sprint 9: Approval Rule Engine, Audit Log, Memory Hot-Reload, Plugin RPC, & TUI/CLI
> **Tujuan:** Menyelesaikan sistem keamanan 3 sumbu, rotasi audit, memory forget/mtime, plugin JSON-RPC, dan slash commands.

- [x] **9.1 Approval Gate 3 Sumbu & Audit Logger (`crates/purrcode-approval/`)**
  - Evaluasi `deny > ask > allow`, penyaringan `[FILTERED]`, dan penulisan `audit.jsonl` dengan rotasi ukuran (teruji).
- [x] **9.2 Memory `forget` & `mtime` Hot-Reload (`crates/purrcode-memory/`)**
  - Pemuatan `SOUL.md`, `USER.md`, `MEMORY.md`, dan `.purrcode/MEMORY.md` beserta penghapusan fakta `forget` (teruji).
- [x] **9.3 Plugin Manager JSON-RPC & Capability Default-Deny (`crates/purrcode-plugin/`)**
  - Parser `plugin.yaml`, isolasi secret environment (`scrub_plugin_env`), dan lifecycle hooks (teruji).
- [x] **9.4 TUI Ask Picker, Slash Commands, Embedded Skills, & Subcommand CLI**
  - Widget `AskPromptRequest` di Ratatui, slash commands TUI, 8 skill bawaan ter-embed di binary, dan subcommand CLI `preset`, `model-preset`, `session`, `plugin`, `service`, `exec` (teruji).
