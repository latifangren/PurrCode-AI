# PurrCode AI — Panduan Eksekusi & Bukti Uji (ROADMAP_SPRINT.md)

- **Status Terakhir:** Seluruh Sprint 1 s/d 9 (Milestone v0.1) selesai dan terverifikasi 100% (68 tests passing di seluruh 12 crate).
- **Audit Komprehensif:** Lihat `docs/AUDIT_STATUS.md` untuk perbandingan mendalam terhadap Claude Code, Codex, OMP, Hermes Agent, dan Muse.
- **Timestamp Audit:** `2026-11-11 22:05:00 +07:00`

Dokumen ini mencatat status implementasi tiap sprint beserta nama unit test eksak yang memverifikasinya di `cargo test --workspace`.

---

## Sprint 1: One-Shot LLM CLI (`purrcode-ai -p "<prompt>"`)
> **Tujuan:** Binary membaca config custom endpoint, memvalidasi skema, dan mengalirkan respon streaming ke terminal.

- [x] **1.1 Config Loader (`crates/purrcode-config/`)**
  - Stripper komentar & trailing comma JSONC (`jsonc::tests::test_clean_jsonc_comments`, `jsonc::tests::test_strip_trailing_commas`).
  - Interpolasi variabel environment & credentials (`loader::tests::test_expand_env_vars`).
  - Parsing `purrcode.jsonc.example` (`loader::tests::test_parse_example_file`).
- [x] **1.2 OpenAI-Compatible Provider (`crates/purrcode-provider/`)**
  - Klien SSE `POST /v1/chat/completions` dengan dukungan delta teks, reasoning (`thought`/`reasoning_content`), `reasoning_effort`, dan pelacakan `is_healthy()`.
- [x] **1.3 Simple CLI One-Shot (`crates/purrcode-cli/`)**
  - Perintah `purrcode-ai -p "<prompt>"`, `--json` (NDJSON), dan startup guard yang menolak jalan bila belum ada model dikonfigurasi.

---

## Sprint 2: 12 Tools Inti & Jembatan MCP
> **Tujuan:** Agent loop memanggil tool, mengeksekusi aksi file/bash di dalam batas workspace, dan menghubungkan server MCP.

- [x] **2.1 File, Search & Execution Tools (`crates/purrcode-tools/`)**
  - `read`: Baca file ber-window + hash FNV-1a + confinement (`fs::read::tests::test_read_tool`, `fs::read::tests::test_read_confinement_blocks_traversal`).
  - `write`: Tulis file atomik + dry-run diff preview.
  - `edit`: Replace exact match + hash-anchoring `expected_hash` + unified diff preview (`fs::edit::tests::test_edit_unique_match_and_hash_anchor`).
  - `bash`: Eksekusi shell dengan timeout dan `kill_on_drop(true)`.
  - `grep`, `find`, `glob`: Pencarian file terbatas (`search::glob::tests::test_glob_to_regex`).
- [x] **2.2 Approval Gate & Audit (`crates/purrcode-approval/`)**
  - Evaluasi `deny > ask > allow` + penyempitan policy child (`tests::test_deny_beats_ask_beats_allow_and_narrowing`).
  - Scrubbing secret `[FILTERED]` dan rotasi `audit.jsonl` (`tests::test_secret_scrubbing_and_audit_rotation`).
- [x] **2.3 MCP Client Bridge (`crates/purrcode-tools/src/mcp/`)**
  - Spawner stdio JSON-RPC 2.0 dengan environment scrubbing (`scrub_sensitive_env`) dan tier `ApprovalTier::Exec`.
- [x] **2.4 Agent Loop Engine (`crates/purrcode-core/`)**
  - Multi-turn loop dengan dry-run diff preview ke `ApprovalRequester` dan pencatatan `AuditSink` (`loop_engine::tests::test_dry_run_diff_preview_and_audit_sink`, `loop_engine::tests::test_policy_gate_deny_blocks_tool_and_audits`).

---

## Sprint 3: TUI Terminal Interaktif (Ratatui)
> **Tujuan:** Pengalaman terminal dengan card tool-call, preview diff, picker `ask`, dan prompt interaktif `purr>`.

- [x] **3.1 TUI State, Layout & Event Handling (`crates/purrcode-channel/src/tui/`)**
  - Differential rendering Ratatui + Crossterm, UTF-8 input editing, dan penanganan `TuiEvent` (`tui::app::tests::test_tui_app_utf8_editing_and_events`).
- [x] **3.2 Widgets Khusus & Card Tools**
  - Render transcript, card status tool (`[running]`, `[done]`, `[failed]`), approval card (`[a]`/`[t]`/`[e]`), dan picker `AskPromptRequest`.
- [x] **3.3 Interupsi (`Esc` / `cancel_flag`) & Mid-Turn Steering (`SteeringQueue`)**
  - `Esc` saat generasi membatalkan turn aktif via `cancel_flag`; input susulan saat generasi masuk ke `SteeringQueue`.

---

## Sprint 4: Meowstro & Subagents (The Clowder)
> **Tujuan:** Orchestrator read-only dan subagent anti-hilang-konteks.

- [x] **4.1 Meowstro Engine (`crates/purrcode-orchestrator/`)**
  - Penegakan toolset Meowstro di level runtime (`read`, `grep`, `glob`, `find`, `todo`, `ask`, `read_skill`, `task` — tanpa `write`, `edit`, `bash`) dan pembekuan `ToolRegistry` (`tool::tests::test_tool_registry_register_retain_and_freeze`).
- [x] **4.2 Continuation Registry, Handoff Files, & Tuxeshadow Auditor**
  - Pembuatan ID stabil `purr_<id>`, persistensi `subagents/<id>.json`, `history.json`, `brief.md`, `result.md` tervalidasi skema, dan `workgraph.json` (`task::tests::test_subagent_chain_handoff_and_resume_by_id`).
  - Mesin `ShadowAuditor` (`Tuxeshadow`): trigger sempit, parsing verdict (`APPROVE`/`APPROVE_WITH_NOTES`/`OBJECT`), dan penegakan per mode `Strict`/`Default`/`God` (`shadow::tests::test_parse_shadow_verdicts_and_triggers`, `shadow::tests::test_shadow_enforcement_across_modes`).
- [x] **4.3 Memori Persisten Markdown (`crates/purrcode-memory/`)**
  - Pemuatan `SOUL.md`, `USER.md`, `MEMORY.md`, `.purrcode/MEMORY.md` serta `remember`, `recall`, dan `forget` berbasis ID fakta eksak (`tests::test_markdown_memory_remember_recall_and_forget`).

---

## Sprint 5: Sisa Tool Inti & Skill Engine (Agentskills)
> **Tujuan:** Menyelesaikan 12 tool inti v0.1 dan pemuatan skill berbasis `SKILL.md`.

- [x] **5.1 Tool Coordination (`crates/purrcode-tools/src/coordination/`)**
  - `todo`: Pelacak progres rencana kerja (`coordination::todo::tests::test_todo_tool`).
  - `ask`: Dialog keputusan terstruktur via `AskRequester` / TTY / non-interaktif (`coordination::ask::tests::test_ask_non_interactive`, `coordination::ask::tests::test_ask_empty_options`).
- [x] **5.2 Tool Web (`crates/purrcode-tools/src/web/`)**
  - `webfetch`: HTTP/HTTPS GET terbatas 1 MB dengan proteksi SSRF literal IP, redirect, dan resolusi DNS (`web::webfetch::tests::test_ssrf_blocking`, `web::webfetch::tests::test_utf8_safe_truncation`).
  - `websearch`: Pencarian web dengan propagasi error jaringan/HTTP (`web::websearch::tests::test_websearch_propagates_network_error`, `web::websearch::tests::test_extract_snippets_and_encode`).
- [x] **5.3 Skill Engine (`crates/purrcode-tools/src/skill/` & `skills/`)**
  - 8 skill bawaan v0.1 ter-embed di binary + discovery hierarkis (`skill::tests::test_embedded_skills_always_available`).

---

## Sprint 6 – 9: Konfigurasi Berlapis, Preset, Router, Session Store, Plugin Hook, & Onboarding
> **Tujuan:** Menyelesaikan kontrak konfigurasi, preset, fallback router, session store, plugin hook, dan onboarding.

- [x] **6.1 Onboarding & Validasi Konektivitas (`crates/purrcode-cli/src/onboard.rs`)**
  - Penyimpanan kredensial `0600` (`onboard::tests::test_save_credential_safe`), serialisasi config aman (`onboard::tests::test_config_serialization_escaping`), dan validasi konektivitas HTTP (`onboard::tests::test_validate_endpoint_connectivity_rejects_dead_endpoint`).
- [x] **7.1 Hierarchical Config Merge & 64K Context Gate (`crates/purrcode-config/`)**
  - Merge berjenjang (`loader::tests::test_hierarchical_merge`) dan penolakan model `< 64K` (`validator::tests::test_rejects_model_under_64k`).
- [x] **7.2 Mode Preset (`preset/v1`) & Model Preset (`model-preset/v1`) + Resolusi Per Role**
  - Loader `presets/*.jsonc` (`preset_mode::tests::test_bundled_mode_presets_load`), `model-presets/*.jsonc` (`preset_model::tests::test_bundled_model_presets_load`), dan resolusi model per role (`task::tests::test_per_role_model_preset_resolution_and_precedence`).
- [x] **7.3 Fallback Router & `ProviderRefusal` (`crates/purrcode-provider/`)**
  - Penghentian rantai fallback saat `PurrError::ProviderRefusal` (`router::tests::test_policy_refusal_stops_fallback_chain`).
- [x] **8.1 Session Store & Compaction (`crates/purrcode-core/src/session.rs`)**
  - Append-only JSONL, fork, export, dan compaction non-destruktif (`session::tests::test_session_append_fork_and_compact`).
- [x] **9.1 Plugin Manager & Hook Veto (`crates/purrcode-plugin/` & `crates/purrcode-core/`)**
  - Parser `plugin.yaml` & default-deny secret (`tests::test_parse_plugin_yaml_and_default_deny_secrets`) serta veto `pre_tool_use` di `AgentLoop` (`loop_engine::tests::test_plugin_pre_tool_use_veto_blocks_execution`).
- [x] **9.2 Service Unit Generators (`crates/purrcode-service/`)**
  - Generator unit systemd & OpenRC untuk `purrcode-ai gateway` (`tests::test_service_unit_generators`).
