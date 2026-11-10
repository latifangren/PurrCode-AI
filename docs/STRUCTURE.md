# PurrCode AI — Struktur Folder, Modul & Kode (STRUCTURE.md)

Versi: 0.1 — 2026-11-11  
Status: Spesifikasi layout codebase, modularitas, dan konvensi file.  
Bahasa: **Rust** (Cargo workspace). Lisensi: **GPL-3.0**.  
Binary: **`purrcode-ai`**.

---

## 1. Prinsip Modularitas & Konvensi Kode

### 1.1 Strictly Modular (Single Responsibility)
- Setiap crate hanya memiliki satu domain tanggung jawab.
- Tidak ada modul raksasa serba ada (*god module*).
- Komunikasi antar subsistem selalu melalui kontrak trait di `purrcode-api` atau lewat `EventBus` bertipe, bukan memanggil implementasi konkret secara erat.

### 1.2 Zero Hardcode
- **Model & Endpoint:** Dilarang ada URL provider, API key, atau model default yang di-hardcode ke dalam binary. Semua endpoint dan model ID dibaca dari `~/.purrcode/purrcode.jsonc` (atau `.purrcode/purrcode.jsonc` per proyek) atau argumen CLI.
- **Preset & Guardrail:** Diatur oleh file JSONC di `presets/*.jsonc` (`preset/v1`).
- **Mix-Model Subagent:** Diatur oleh file JSONC di `model-presets/*.jsonc` (`model-preset/v1`).
- **Prompt & Agen:** Diatur oleh file Markdown + frontmatter di `.purrcode/agents/<nama>.md`.
- **Tools List:** Didaftarkan secara dinamis di `ToolRegistry`.

### 1.3 Batas Baris Kode (File Size Limit)
- **Hard Limit: 800–1000 baris per file.**
- Sebelum menyentuh 1000 baris, file **wajib** di-refactor / di-split menjadi submodul direktori (misal `session.rs` di-split menjadi `session/mod.rs`, `session/store.rs`, `session/compaction.rs`, `session/types.rs`).
- Pengecualian: Boleh 1000 lebih sedikit hanya bila pemecahan akan merusak kohesi logika yang sangat erat (misal *state machine transition table* murni atau *lexer pattern match table*), dan harus dicatat alasannya.

### 1.4 Rantai Crate Satu Arah (One-Way Dependency DAG)
Ditegakkan agar tidak terjadi dependensi melingkar (*circular dependency*):
```text
purrcode-cli (binary purrcode-ai)
   │
   ├───────────────┬────────────────┬───────────────┐
   ▼               ▼                ▼               ▼
purrcode-channel  purrcode-orchestrator  purrcode-plugin  purrcode-service
   │               │                │               │
   ▼               ▼                ▼               │
purrcode-tools    purrcode-memory  purrcode-approval│
   │               │                │               │
   └───────────────┼────────────────┴───────────────┘
                   ▼
             purrcode-provider
                   │
                   ▼
             purrcode-core
                   │
                   ▼
             purrcode-config
                   │
                   ▼
             purrcode-api (leaf contracts & traits)
```

---

## 2. Struktur Direktori Root

```text
PurrCode-AI/
├── Cargo.toml                  # Root Cargo workspace manifest
├── Cargo.lock                  # Lockfile dependensi
├── .cargo/
│   └── config.toml             # Target build, rustflags, zigbuild alias
├── .gitignore                  # Filter ketat (target/, references/, .purrcode/, dll.)
├── AGENTS.md                   # Aturan kerja agen & manusia di repo ini
├── README.md                   # Ringkasan proyek & status
├── LICENSE                     # GNU GPL v3.0
├── purrcode.jsonc.example      # Contoh konfigurasi lengkap
├── agents/                     # Definisi 7 agen The Clowder (.md + frontmatter)
├── skills/                     # 8 skill bawaan v0.1 (SKILL.md + kontrak routing)
├── docs/                       # Dokumentasi arsitektur & desain
│   ├── 00-analysis-references.md # Analisis repo referensi & lisensi
│   ├── PRD.md                  # Product Requirements Document (v0.3)
│   ├── ARCHITECTURE.md         # Spesifikasi arsitektur teknis
│   ├── DESIGN.md               # Desain TUI, UX, dan ADR
│   ├── STRUCTURE.md            # File ini (panduan folder & file)
│   ├── DECISIONS.md            # Catatan ADR terpusat (ADR-001 s/d ADR-012)
│   ├── DEPENDENCIES.md         # Audit lisensi dependensi GPL-3.0
│   ├── SUBAGENTS.md            # Kontrak protokol handoff & continuation ID
│   ├── MCP.md                  # Spesifikasi klien MCP stdio/HTTP
│   ├── LSP.md                  # Strategi code intelligence & LSP
│   └── ROADMAP_SPRINT.md       # Checklist eksekusi sprint
├── presets/                    # Mode presets bawaan (preset/v1)
│   ├── carrier.jsonc           # Strict mode
│   ├── tabby.jsonc             # Default supervised mode
│   └── oyen.jsonc              # God mode (guardrail agen off)
├── model-presets/              # Model presets bawaan (model-preset/v1)
│   ├── custom-endpoint.jsonc   # Template proxy lokal/LAN (Magpie/OmniRoute)
│   ├── free.jsonc              # Template tier gratis
│   ├── local.jsonc             # Template Ollama lokal offline
│   └── premium.jsonc           # Template flagship BYO key
├── crates/                     # Workspace crates (Rust)
│   ├── purrcode-api/           # Trait bersama & tipe kontrak
│   ├── purrcode-config/        # Parser JSONC, validasi skema, konfigurasi
│   ├── purrcode-core/          # Event bus, agent loop, session store
│   ├── purrcode-provider/      # Adapter LLM (custom OpenAI-compatible, Ollama, dll.)
│   ├── purrcode-approval/      # Security rule engine, gate, audit log
│   ├── purrcode-tools/         # 12 tool inti v0.1 + tool registry
│   ├── purrcode-memory/        # MarkdownMemory, trait MemoryBackend, recall
│   ├── purrcode-orchestrator/  # Meowstro DAG, delegasi subagent, handoff files
│   ├── purrcode-plugin/        # Plugin manager, JSON-RPC stdio transport, lifecycle
│   ├── purrcode-channel/       # TUI (Ratatui), NDJSON stdout, abstraction channel
│   ├── purrcode-service/       # Systemd & OpenRC service installer
│   └── purrcode-cli/           # Binary main entry purrcode-ai (Clap CLI)
└── references/                 # CLONE READ-ONLY (gitignored)
```

---

## 3. Rincian Modul & File Tiap Crate

### 3.1 `crates/purrcode-api/` (Leaf — Kontrak & Trait)
Hanya berisi definisi tipe publik, error bersama, dan trait. **Nol logika bisnis internal.**
```text
crates/purrcode-api/
├── Cargo.toml
└── src/
    ├── lib.rs              # Re-export seluruh trait & tipe kontrak
    ├── error.rs            # PurrError, Result bersama (< 300 baris)
    ├── event.rs            # Definisi event bertipe untuk EventBus (AgentEvent, TurnEvent)
    ├── provider.rs         # Trait ModelProvider, CompletionRequest, StreamChunk, TokenUsage
    ├── tool.rs             # Trait Tool, ToolDefinition, ToolCall, ToolOutput, ApprovalTier
    ├── memory.rs           # Trait MemoryBackend, Fact, MemoryEntry
    ├── channel.rs          # Trait Channel, ChannelCapabilities, UserInput
    └── plugin.rs           # Trait Plugin, PluginManifest, PluginCapabilities, HookEvent
```

### 3.2 `crates/purrcode-config/` (Konfigurasi & Skema)
Menangani loading, merging berjenjang, dan validasi file config JSONC.
```text
crates/purrcode-config/
├── Cargo.toml
└── src/
    ├── lib.rs              # Entry point purrcode-config
    ├── model.rs            # Struct Config utama, ProviderConfig, ModelSpec (< 400 baris)
    ├── loader.rs           # Hierarchical loader (system -> ~/.purrcode -> .purrcode)
    ├── parser.rs           # JSONC parser dengan proteksi komentar & trailing comma
    ├── preset_mode.rs      # Skema & loader preset/v1 (carrier, tabby, oyen)
    ├── preset_model.rs     # Skema & loader model-preset/v1 (extends, per-agent mapping)
    └── validator.rs        # Validasi batas konteks minimal 64K, format path, dsb.
```

### 3.3 `crates/purrcode-core/` (Jantung Eksekusi)
Event bus murni, agent loop berulang, dan penyimpanan sesi.
```text
crates/purrcode-core/
├── Cargo.toml
└── src/
    ├── lib.rs              # Entry point purrcode-core
    ├── bus.rs              # EventBus broadcast async (tokio::sync::broadcast)
    ├── loop_engine.rs      # Agent loop murni (run_turn, max_steps guard, interrupt) (< 600 baris)
    ├── context_builder.rs  # Penyusun prompt (system + memory + history + tools)
    ├── session/
    │   ├── mod.rs          # SessionManager facade
    │   ├── store.rs        # Dual-store: append-only JSONL + SQLite session index
    │   ├── model.rs        # Session, Turn, Message types
    │   └── compaction.rs   # Non-destructive compaction (reserve recent tokens)
    └── steering.rs         # Steering queue (pesan susulan user di tengah turn)
```

### 3.4 `crates/purrcode-provider/` (Adapter LLM & Routing)
Adapter ke provider API eksternal dan custom endpoint/proxy.
```text
crates/purrcode-provider/
├── Cargo.toml
└── src/
    ├── lib.rs              # ProviderFactory & chokepoint pembuatan client
    ├── error.rs            # ProviderError, ProviderRefusal (ToS policy failure)
    ├── custom_openai.rs    # OpenAI-compatible adapter (Magpie, OmniRoute, vLLM, Ollama v1)
    ├── ollama_native.rs    # Ollama native API adapter (tags check, local status)
    ├── anthropic.rs        # Anthropic Claude Messages API adapter
    ├── gemini.rs           # Google Gemini API adapter
    ├── router.rs           # Fallback chain runner + cooldown/rate limiter (< 500 baris)
    └── stream_parser.rs    # SSE parser normalizer untuk tool calls & text delta
```

### 3.5 `crates/purrcode-approval/` (Keamanan & Audit)
Rule engine evaluasi izin aksi dan audit trail yang tidak bisa dimatikan.
```text
crates/purrcode-approval/
├── Cargo.toml
└── src/
    ├── lib.rs              # ApprovalGate facade
    ├── engine.rs           # Rule engine: deny > ask > allow (< 400 baris)
    ├── policy.rs           # ApprovalMode (manual, supervised, auto, yolo) & GuardrailMode
    ├── confinement.rs      # Workspace directory confinement checker
    ├── audit.rs            # AuditLogger: append-only audit.jsonl, scrub secret [FILTERED]
    └── rotation.rs         # Rotasi audit log per ukuran + kompresi gz
```

### 3.6 `crates/purrcode-tools/` (12 Tool Inti & Registry)
Implementasi 12 tool bawaan v0.1 yang terisolasi rapi.
```text
crates/purrcode-tools/
├── Cargo.toml
└── src/
    ├── lib.rs              # ToolRegistry builder & default toolset loader
    ├── registry.rs         # Registry map: name -> Arc<dyn Tool>, freeze check
    ├── fs/
    │   ├── mod.rs          # Re-export fs tools
    │   ├── read.rs         # Tool `read` (bounded offset/limit)
    │   ├── write.rs        # Tool `write` (atomic file write)
    │   ├── edit.rs         # Tool `edit` (hash-anchored exact find/replace + preview)
    │   └── find.rs         # Tool `find` (filename discovery)
    ├── search/
    │   ├── mod.rs          # Search tools
    │   ├── grep.rs         # Tool `grep` (ripgrep-backed bounded regex/literal)
    │   └── glob.rs         # Tool `glob` (file pattern match)
    ├── execution/
    │   ├── mod.rs          # Bash execution tool
    │   └── bash.rs         # Tool `bash` (timeout, background detection, sandboxed)
    ├── coordination/
    │   ├── mod.rs          # Task, Ask, Todo
    │   ├── ask.rs          # Tool `ask` (structured multi-choice UI picker)
    │   ├── todo.rs         # Tool `todo` (plan tracker: pending/in_progress/completed)
    │   └── task.rs         # Tool `task` (spawn/resume subagent continuation ID)
    └── web/
        ├── mod.rs          # Web fetch & search
        ├── webfetch.rs     # Tool `webfetch` (bounded scrape/read)
        └── websearch.rs    # Tool `websearch` (bounded search queries)
```

### 3.7 `crates/purrcode-memory/` (Memori Persisten Markdown)
Default minimal tanpa database wajib, recall grep, hot-reload mtime.
```text
crates/purrcode-memory/
├── Cargo.toml
└── src/
    ├── lib.rs              # MemoryService facade
    ├── markdown.rs         # MarkdownMemory (baca/tulis MEMORY.md, USER.md, SOUL.md) (< 500 baris)
    ├── watcher.rs          # Hot-reload via file mtime tracking
    ├── recall.rs           # Simple keyword/grep recall engine over markdown
    └── fts_sqlite.rs       # Plugin opsional: SQLite FTS5 backend (bila diaktifkan)
```

### 3.8 `crates/purrcode-orchestrator/` (Meowstro & The Clowder)
Orchestration graph, koordinasi subagent, dan pencegahan hilang konteks.
```text
crates/purrcode-orchestrator/
├── Cargo.toml
└── src/
    ├── lib.rs              # Orchestrator facade
    ├── meowstro.rs         # Meowstro orchestrator loop (read-only + delegation guard) (< 500 baris)
    ├── clowder.rs          # The Clowder team definitions (Whisker, Sphinx, Calico, Tinker, Tuxeshadow, Clowder)
    ├── subagent/
    │   ├── mod.rs          # Subagent runner
    │   ├── continuation.rs # Continuation ID generator & resolver (purr_<id>)
    │   ├── handoff.rs      # Handoff files generator: brief.md & result.md
    │   └── registry.rs     # Subagent state registry (subagents/<id>.json)
    └── workgraph.rs        # DAG task scheduler & serialisasi workgraph.json
```

### 3.9 `crates/purrcode-plugin/` (Ekstensibilitas & Plugin RPC)
Pemuatan plugin bawaan (Cargo feature) dan plugin eksternal (proses JSON-RPC stdio).
```text
crates/purrcode-plugin/
├── Cargo.toml
└── src/
    ├── lib.rs              # PluginManager facade
    ├── manifest.rs         # Parser plugin.yaml & capability verification
    ├── transport_process.rs# JSON-RPC 2.0 stdio client over child process (< 600 baris)
    ├── hooks.rs            # Hook dispatcher (pre_tool_use, post_tool_use, on_start, on_stop)
    └── isolation.rs        # Environment scrubber (hapus secret sebelum spawn proses plugin)
```

### 3.10 `crates/purrcode-channel/` (TUI Terminal & CLI Channels)
Renderer UI di atas EventBus bersama.
```text
crates/purrcode-channel/
├── Cargo.toml
└── src/
    ├── lib.rs              # Channel registry & event listener
    ├── ndjson.rs           # Headless event stream (--json / -p)
    └── tui/
        ├── mod.rs          # Ratatui TUI main event loop (< 500 baris)
        ├── app.rs          # TUI App state
        ├── layout.rs       # Terminal layout & splitters
        ├── theme.rs        # Color theme & cat palette (NO_COLOR compliant)
        ├── widgets/
        │   ├── transcript.rs # Streaming message & reasoning renderer
        │   ├── tool_card.rs  # Tool call & result card display
        │   ├── edit_preview.rs # Diff preview card with [a]/[t]/[e] actions
        │   ├── subagent_bar.rs# Active subagents indicator (Whisker, Sphinx, Tinker)
        │   ├── ask_picker.rs # Structured multi-choice interactive widget
        │   └── status_bar.rs # Model, preset, token & session meter
        └── keybinds.rs     # Key event router (Ctrl-C interrupt, Tab, Esc)
```

### 3.11 `crates/purrcode-service/` (Daemon & Service Manager)
Instalasi service systemd (Linux) dan OpenRC (router/STB).
```text
crates/purrcode-service/
├── Cargo.toml
└── src/
    ├── lib.rs              # ServiceManager
    ├── systemd.rs          # Generator & installer unit systemd
    └── openrc.rs           # Generator init script OpenRC
```

### 3.12 `crates/purrcode-cli/` (Binary Entry: `purrcode-ai`)
Parser CLI argumen via Clap dan bootstrap runtime.
```text
crates/purrcode-cli/
├── Cargo.toml
└── src/
    ├── main.rs             # fn main() -> Result<()> (< 200 baris)
    ├── cli.rs              # Clap CLI subcommand & flags definition
    ├── commands/
    │   ├── mod.rs          # Subcommand dispatch
    │   ├── agent.rs        # Default interactive TUI / run prompt
    │   ├── onboard.rs      # Interactive setup wizard (provider/endpoint/model)
    │   ├── doctor.rs       # purrcode-ai doctor diagnostics
    │   ├── preset.rs       # purrcode-ai preset list/show
    │   ├── model_preset.rs # purrcode-ai model-preset list/show
    │   ├── session.rs      # purrcode-ai session list/export/resume
    │   ├── plugin.rs       # purrcode-ai plugin list/install
    │   └── service.rs      # purrcode-ai service install/status
    └── banner.rs           # ASCII cat & banner version
```

---

## 4. Panduan Implementasi Bertahap (Roadmap Code)

1. **Fase Scaffold Awal:**
   - Setup root `Cargo.toml` workspace dengan seluruh crate skeleton.
   - Setup `purrcode-api` untuk semua trait dasar.
   - Setup `purrcode-config` untuk membaca JSONC custom endpoint & preset.
2. **Fase Core & CLI One-Shot (0.1a):**
   - Implementasi `purrcode-provider` custom OpenAI-compatible endpoint.
   - Implementasi `purrcode-core` (loop dasar) dan `purrcode-cli -p "<prompt>"`.
3. **Fase Tools & TUI (0.1b):**
   - Implementasi 12 tool di `purrcode-tools`.
   - Implementasi Ratatui di `purrcode-channel`.
4. **Fase Subagent & Orchestrator (0.1c):**
   - Implementasi Meowstro DAG & continuation ID di `purrcode-orchestrator`.
