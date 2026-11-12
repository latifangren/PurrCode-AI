# PurrCode AI 🐾

[![License: GPL-3.0](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.80%2B-orange.svg)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/Platform-Linux%20%7C%20macOS%20%7C%20Windows-brightgreen.svg)]()
[![Workspace Crates](https://img.shields.io/badge/Crates-12%20Modular-blueviolet.svg)]()

> **The Sovereign Multi-Agent AI Engineer in Pure Rust.**  
> Menggabungkan arsitektur orkestrasi subagen **The Clowder** (*oh-my-opencode-slim*) dengan ketangguhan dan otonomi sistemik **Hermes Agent** (*multi-channel gateway, prompt caching invariant, markdown identity memory, autonomous cron, dan self-improving curator*).

---

## 🌟 Sorotan Fitur Utama

- 🦀 **Single Static Musl Binary (`purrcode-ai`)**: Ditulis 100% dalam Rust 1.80+, tanpa runtime Python/Node.js, tanpa database eksternal, startup instan (< 15ms), dan konsumsi RAM sangat hemat.
- 🐈 **The Clowder Multi-Agent Architecture**:
  - **Meowstro (The Orchestrator)**: Terkunci pada invariant *read-only* murni (`read`, `grep`, `glob`, `find`, `todo`, `ask`, `task`, `read_skill`). Meowstro **tidak pernah** menulis atau mengedit file langsung.
  - **Tinker (The Implementer/Fixer)**: Spesialis pelaksana mandiri dengan hak akses penuh (`write`, `edit`, `bash`, `manage_skill`).
  - **Whisker (The Test Runner)**: Eksekusi rangkaian uji, verifikasi regresi, dan diagnosa test runner.
  - **Sphinx (The Architect)**: Analisis dependensi tingkat dalam, audit arsitektural, dan evaluasi trade-off.
  - **Calico (The Diff Reviewer)**: Review kode berstandar tinggi, verifikasi format diff, dan deteksi regresi.
  - **Tuxeshadow (Shadow Auditor)**: Auditor bayangan strictly *fail-closed* yang memvalidasi `plan_json` dan `diff_preview` sebelum eksekusi dijalankan.
- 🔄 **Continuation ID & Zero Context Loss**:
  - Setiap subagen memiliki ID persisten (`purr_<id>`) dengan prinsip *"amend child, never respawn"*.
  - Artefak handoff terstruktur otomatis (`brief.md` + `result.md`).
  - Batasan kedalaman rekursi ketat depth = 1 (mencegah loop tak terhingga).
- ⚡ **Hermes Agent Invariants**:
  - **Byte-Exact Prompt Caching**: Daftar tool dan pesan sistem diurutkan secara deterministik untuk menjamin cache hit maksimal pada provider LLM.
  - **SOUL / USER / MEMORY Identity**: Profiling identitas berbasis Markdown dengan batas karakter ketat (`SOUL.md`, `USER.md` [1375 char], `MEMORY.md` [2200 char]).
  - **Autonomous Cron Scheduler**: Parser interval berbasis bahasa alami (`"every 15m"`, `"every 2 hours"`, `"daily at 09:00"`) dengan ticker latar belakang 60 detik.
  - **Self-Improving Skill Curator**: Perekaman telemetri otomatis, deteksi keusangan skill (> 14 hari `Stale`), dan pengarsipan otomatis (> 30 hari ke `archive/`).
  - **Telegram Remote Gateway**: Channel bot Telegram interaktif dengan tombol remote approval (`allow:` / `deny:`) dan otorisasi `allowed_users`.
- 🛡️ **Fail-Closed Security & Path Confinement**:
  - Kebijakan izin supervisi: `deny > ask > allow`. Deny selalu menang.
  - Timeout persetujuan 180 detik otomatis default ke `deny` (*fail-closed*).
  - Pembatasan path kanonikal ketat pada `workspace_root` (mencegah *directory traversal* `..`).
  - Mitigasi SSRF pada tool web (`webfetch`, `websearch`) dengan proteksi IP privat, loopback, link-local, dan CGNAT.
- 🎛️ **Mode Presets (`preset/v1`)**:
  - `carrier` (Strict): Memerlukan persetujuan pada seluruh aksi berisiko, objection Tuxeshadow memblokir eksekusi.
  - `tabby` (Default Supervised): Persetujuan pada tool mutasi (`write`, `edit`, `bash`), Tuxeshadow bersifat advisory dengan override yang tercatat di audit trail.
  - `oyen` (God Mode): Otonom tanpa konfirmasi, bypass supervisi untuk workflow CI/CD otomatis.

---

## 🏗️ Struktur Ruang Kerja (Workspace)

Repositori ini terbagi menjadi 12 crate modular dengan rantai dependensi satu arah yang ketat:

```text
purrcode-api           (Kontrak dasar, error types, traits Tool & ToolContext, AgentEvent)
  ↓
purrcode-config        (Hierarchical config loader, JSONC parser, mode & model presets)
  ↓
purrcode-core          (AgentLoop engine, steering queue, cancellation flags, event bus)
  ↓
purrcode-provider      (OpenAI-compatible streaming, Ollama, vLLM, reasoning tokens, fallback router)
  ↓
[ Domain Crates ]
  ├─ purrcode-tools        (13 tools inti: fs, exec, search, coord, web, skill + MCP client)
  ├─ purrcode-memory       (Markdown-based persistent memory: MEMORY.md, USER.md, SOUL.md)
  ├─ purrcode-approval     (ApprovalGate fail-closed, policy tiers, regex rules)
  └─ purrcode-plugin       (Plugin manifest loader, sandbox default-deny, hook dispatcher)
  ↓
[ Upper Layer Crates ]
  ├─ purrcode-orchestrator (The Clowder multi-agent coordinator, Meowstro, TaskTool, Tuxeshadow)
  ├─ purrcode-channel      (TUI terminal Ratatui & Telegram bot remote channel)
  └─ purrcode-service      (Autonomous Cron scheduler, Curator engine, systemd/OpenRC/Windows installer)
  ↓
purrcode-cli           (Aplikasi biner utama: Clap CLI, onboard wizard, doctor, gateway daemon)
```

---

## 🚀 Instalasi & Kompilasi

### Prasyarat
- Rust 1.80+ (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- Git

### Kompilasi Release Binary
```bash
git clone https://github.com/latifangren/PurrCode-AI.git
cd PurrCode-AI

# Kompilasi binary release teroptimasi
cargo build --release

# Binary terletak di target/release/purrcode-ai
./target/release/purrcode-ai --help
```

---

## 📖 Panduan Penggunaan CLI

### 1. Diagnostik & Setup Awal
```bash
# Jalankan wizard onboarding interaktif
purrcode-ai onboard

# Diagnosa konektivitas provider, model, dan lingkungan
purrcode-ai doctor
```

### 2. Mode Terminal Interaktif (Ratatui TUI)
```bash
# Menjalankan TUI standar
purrcode-ai

# Menjalankan TUI dengan orkestrasi The Clowder (Meowstro)
purrcode-ai --orchestrator

# Menggunakan mode preset tertentu (carrier, tabby, oyen)
purrcode-ai --preset carrier

# Menggunakan model preset tertentu (free, local, premium, custom-endpoint)
purrcode-ai --model-preset premium
```

### 3. Eksekusi Non-Interaktif (One-Shot Prompt)
```bash
# Prompt langsung sekali jalan
purrcode-ai -p "Periksa apakah ada syntax error di src/main.rs"

# Menjalankan orkestrator Meowstro dalam sekali jalan
purrcode-ai --orchestrator -p "Buat modul auth baru dan uji unit test-nya"

# Output streaming dalam format JSON (NDJSON untuk scripting / CI)
purrcode-ai --json -p "Hitung 2 + 2"

# Mode otomatis tanpa persetujuan (YOLO)
purrcode-ai --yolo -p "Format kode dan rapikan cargo clippy"
```

### 4. Background Gateway & Telegram Bot Daemon
```bash
# Menjalankan gateway daemon (cron ticker + telegram polling + HTTP health probe)
purrcode-ai gateway
```
- Endpoint HTTP Health Probe aktif di `http://127.0.0.1:8080/health`.
- Menerima interaksi dan streaming progres langsung dari Telegram.
- Tombol persetujuan eksekusi tool muncul langsung di chat Telegram Anda.

### 5. Pengelolaan Autonomous Cron Jobs
```bash
# Melihat daftar cron job yang terdaftar
purrcode-ai cron list

# Menambahkan tugas berkala dengan bahasa alami dan batasan tool (fail-closed)
purrcode-ai cron add "every 30m" "Periksa log error dan sisa disk space" --tools "read,grep,glob"
purrcode-ai cron add "daily at 02:00" "Jalankan cargo audit dan laporkan dependensi usang"
purrcode-ai cron add "*/15 * * * *" "Sync data upstream repo"

# Menghapus tugas cron
purrcode-ai cron remove <job_id>
```

### 6. Audit, Evaluasi & Promosi Skill (Curator Engine)
```bash
# Evaluasi penggunaan skill berdasarkan telemetri eksekusi
purrcode-ai curator audit

# Melihat daftar candidate skills hasil ekstraksi trajektori agen
purrcode-ai curator candidates

# Menyetujui candidate skill untuk dipromosikan ke pustaka skill aktif
purrcode-ai curator approve <nama_skill>

# Menolak candidate skill
purrcode-ai curator reject <nama_skill>
```

### 7. Sovereign Local WebUI & REST/SSE Server (Milestone v0.4)
```bash
# Menjalankan WebUI server lokal
purrcode-ai web --port 8080

# Menjalankan WebUI dengan otentikasi token untuk akses jarak jauh yang aman
purrcode-ai web --host 0.0.0.0 --port 8080 --token rahasia-kucing-oyen
```
- Akses UI web mandiri di `http://127.0.0.1:8080/` (zero external CDN / static asset dependency).
- Mendukung streaming jawaban LLM via Server-Sent Events (SSE) di `/api/events`.
- Dialog persetujuan interaktif tool call langsung dari browser.
- REST API `/api/health`, `/api/memory`, `/api/skills`, `/api/cron`, `/api/approval`, dan `/api/prompt`.

### 8. Instalasi System Service Daemon
```bash
# Menampilkan unit systemd Linux
purrcode-ai service systemd

# Menampilkan init script OpenRC
purrcode-ai service openrc

# Menampilkan script PowerShell Windows Scheduled Task
purrcode-ai service windows

# Status service
purrcode-ai service status
```

---

## ⚙️ Format Konfigurasi (`purrcode.jsonc`)

PurrCode AI menganut prinsip **Zero Hardcoded Endpoints**. Semua konfigurasi provider, model, dan token didefinisikan secara deklaratif di `~/.purrcode/purrcode.jsonc` atau `.purrcode/purrcode.jsonc`:

```jsonc
{
  "$schema": "./docs/schema/purrcode.schema.json",

  // Behavior mode preset: carrier (strict), tabby (default), oyen (god)
  "preset": "tabby",

  // Subagent inference model preset: custom-endpoint, free, local, premium
  "model_preset": "custom-endpoint",

  // Provider endpoint configurations
  "providers": {
    "custom": {
      "api": "openai-completions",
      "base_url": "${CUSTOM_LLM_BASE_URL}",
      "api_key": "${CUSTOM_LLM_API_KEY}",
      "models": {
        "primary-model": {
          "name": "Primary Orchestrator Model",
          "context_window": 200000,
          "max_tokens": 64000,
          "reasoning": true
        },
        "fast-coder": {
          "name": "Fast Subagent Coding Model",
          "context_window": 131072,
          "max_tokens": 32768,
          "reasoning": true
        }
      }
    }
  },

  // Integrasi Telegram Gateway Bot
  "telegram": {
    "bot_token": "${TELEGRAM_BOT_TOKEN}",
    "allowed_users": ["123456789", "@username"]
  },

  // Konfigurasi Daemon HTTP Gateway
  "gateway": {
    "enabled": true,
    "host": "127.0.0.1",
    "port": 8080
  },

  // Server MCP Eksternal
  "mcp_servers": {
    "playwright": {
      "command": "npx",
      "args": ["-y", "@playwright/mcp@latest"]
    }
  }
}
```

---

## 🛠️ Rangkaian Pengujian & Quality Assurance

PurrCode AI menerapkan standar rekayasa perangkat lunak tanpa kompromi (*zero warnings policy*):

```bash
# Menjalankan seluruh 86 unit test di 23 test suites
cargo test --workspace

# Pemeriksaan linter Rust (zero-warnings policy)
cargo clippy --workspace --all-targets -- -D warnings

# Verifikasi formatting kode
cargo fmt --check
```

---

## 📜 Lisensi & Atribusi

Proyek ini dilisensikan di bawah **GNU General Public License v3.0 (GPL-3.0)**. Lihat berkas [LICENSE](LICENSE) untuk rincian lengkap.

- **Author**: latifangren
- **Arsitektur Referensi**: Terinspirasi oleh kepiawaian modular [oh-my-opencode-slim](https://github.com/alvinunreal/oh-my-opencode-slim) dan keotonomian tangguh [Hermes Agent](https://github.com/NousResearch/Hermes-Agent).
