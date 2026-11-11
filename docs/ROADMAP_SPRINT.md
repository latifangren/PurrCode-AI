# PurrCode AI — Panduan Eksekusi & Bukti Uji (ROADMAP_SPRINT.md)

- **Status Terakhir:** Seluruh Sprint 1 s/d 9 (Milestone v0.1) selesai dan terverifikasi secara mendalam (96 tests lolos di seluruh 12 crate — diukur di Linux, branch `dev-muse`).
- **Audit Komprehensif:** Lihat `docs/AUDIT_STATUS.md` untuk perbandingan mendalam terhadap Claude Code, Codex, OMP, Hermes Agent, dan Muse.
- **Timestamp Audit:** `2026-11-12 04:49:00 +07:00`

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
- [x] **9.2 Service Unit Generators & Windows Task Scheduler (`crates/purrcode-service/`)**
  - Generator unit systemd, OpenRC, dan PowerShell Scheduled Task installer untuk Windows (`tests::test_service_unit_generators`, `tests::test_windows_task_script_generator`).
- [x] **9.3 Windows Non-WSL Architecture (`crates/purrcode-tools/` & `crates/purrcode-cli/`)**
  - Deteksi otomatis Git Bash di Windows dengan fallback transparan ke PowerShell (`execution::bash::tests::test_resolve_windows_shell_detects_git_bash_candidate`, `execution::bash::tests::test_resolve_windows_shell_detects_path_bash`, `execution::bash::tests::test_resolve_windows_shell_falls_back_to_powershell`).
  - Normalisasi prefix verbatim Windows `\\?\` dan perbandingan case-insensitive (`fs::read::tests::test_strip_verbatim_prefix_and_confinement_windows`).
  - Guard codepage konsol Windows UTF-8 (`SetConsoleOutputCP(65001)`).
- [x] **9.4 Resolusi Model Deterministik & TUI Approval Ergonomics (`crates/purrcode-cli/` & `crates/purrcode-channel/`)**
  - Resolusi model sesi deterministik berurutan leksikografis & prioritas (`tests::test_resolve_target_model_deterministic_priority`, `tests::test_resolve_target_model_deterministic_lexicographical_fallback`).
  - Dukungan tombol `y`/`Y`/`Enter` dan `n`/`N` pada dialog approval TUI serta layout kartu approval anti-terpotong.
- [x] **9.5 Resolusi Verifikasi Lanjutan Commit c56d3b7 — branch `dev-muse` (`crates/purrcode-tools/`, `crates/purrcode-config/`, `crates/purrcode-channel/`, `docs/`)**
  - Test PATH Windows benar-benar portabel: helper `windows_join` + 2 unit test (`execution::bash::tests::test_windows_join_uses_backslash_for_windows_paths`, `execution::bash::tests::test_windows_join_keeps_host_separator_for_unix_paths`); `test_resolve_windows_shell_detects_path_bash` kini lolos di Linux.
  - Deterministik langkah-3 `resolve_model` (`model::tests::test_resolve_model_bare_name_is_deterministic_across_providers`).
  - Pemetaan tombol approval TUI diekstrak ke fungsi murni `approval_key_action` + 4 unit test (`tui::tests::test_approval_key_action_approve_keys`, `tui::tests::test_approval_key_action_reject_keys`, `tui::tests::test_approval_key_action_edit_keys`, `tui::tests::test_approval_key_action_other_keys_ignored`).
  - Kejujuran dokumen: angka test 76 → 83 dengan hasil ukur sendiri, "3.7 MB" telanjang dikualifikasi, byte rilis diperbarui saat itu (4.617.256 byte = 4,40 MiB), "Bulletproof" dihapus, musl sebagai target (bukan fakta), dan direktori root `src/` yang kosong dihapus. Workplan branch: `docs/dev-muse-workplan.md`.
- [x] **9.6 Ronde autonomous 2026-11-12 (item 15–17) — `--orchestrator` default + User-Agent + TOCTOU (`crates/purrcode-cli/`, `crates/purrcode-config/`, `crates/purrcode-provider/`, `crates/purrcode-tools/`, `docs/`)**
  - `--orchestrator` default (`-p`/`exec`/TUI) + `--direct` opt-out + label audit jujur (`cli::use_orchestrator`, `cli::agent_role_for` + 2 unit test).
  - User-Agent configurable (`ProviderConfig.user_agent`) + default jujur `PurrCode-AI/0.1` + 1 unit test; live via Magpie: UA Chrome → 200, default PurrCode UA → 200 (koreksi: yang diblokir Cloudflare adalah request TANPA header User-Agent sama sekali — binary lama tidak mengirim UA — bukan literal `reqwest/*`).
  - TOCTOU SSRF tertutup: `resolve_validated_addrs` + `build_pinned_client` (`.resolve_to_addrs`) + `get_pinned` (redirect manual, re-pin per hop) + 4 unit test; `websearch` memakai helper yang sama.
  - Riset referensi (subagent read-only): pola UA pi/codex/picoclaw, norma one-shot (PurrCode menyimpang atas keputusan user + PRD), pinning zeroclaw, capabilities zeroclaw/opencode.
  - Hasil ukur: `cargo test --workspace` 90/90, `cargo build --release` 4.623.400 byte (4,41 MiB).
- [x] **9.7 Ronde autonomous batch 2a 2026-11-12 (item 18–19 + temuan minor N1/N2/R1/R2) — capabilities jujur + light fixes + koreksi UA (`crates/purrcode-provider/`, `crates/purrcode-tools/`, `crates/purrcode-plugin/`, `crates/purrcode-orchestrator/`, `crates/purrcode-channel/`, `crates/purrcode-cli/`, `docs/`)**
  - N1: UA invalid di config divalidasi dulu → kembali ke `DEFAULT_USER_AGENT` (tidak pernah ke client tanpa UA); unit test diperluas dengan kasus UA berisi karakter kontrol.
  - N2: `build_pinned_client` fail-closed (mengembalikan `Result`, error dipropagasi, bukan fallback client polos tanpa pinning); R2: helper `next_hop_url` (validasi target hop sebelum diikuti) + unit test hop redirect ke loopback ditolak.
  - R1: tabel §1 AUDIT_STATUS "83 unit/integration tests" dikoreksi ke 96.
  - Capabilities jujur: `network`/`filesystem` di-parse dari `plugin.yaml` dan dicatat sebagai deklarasi sandbox v0.3; hanya `secrets` yang ditegakkan di v0.1. `ProcessPlugin::new` memberi `tracing::warn!`; `PluginManager::unenforced_capabilities()`; CLI mencatat audit entry `plugin_capability_unenforced` + `purrcode-ai plugin list` menampilkan peringatan `⚠`; klaim "default-deny" di `AGENTS.md`/`CLAUDE.md`/`ARCHITECTURE.md` dikualifikasi ke `secrets` saja. Smoke test live: baris `⚠` tampil di `plugin list` dan 1 entry audit tercatat.
  - Light fixes: dedup heading `## Summary` di `result.md`; range fuzzy `edit` berakhir sebelum newline baris terakhir (LF + CRLF); kartu hasil tool TUI menampilkan 4 baris pertama + marker `… (N more lines)` (diuji via TestBackend).
  - Koreksi rumusan UA di docs: "yang diblokir hanya UA `reqwest/*`" diganti "binary lama tidak mengirim header User-Agent sama sekali".
  - Hasil ukur: `cargo test --workspace` 96/96, `cargo build --release` 4.629.032 byte (4,41 MiB).
