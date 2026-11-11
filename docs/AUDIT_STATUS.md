# PurrCode AI — Status Audit & Laporan Kesiapan Proyek (AUDIT_STATUS.md)

- **Timestamp Audit:** `2026-11-12 05:08:00 +07:00`
- **Lisensi:** GNU GPL-3.0
- **Ukuran Binary Rilis Terverifikasi:** ✅ **~3,7 MB** (`.exe` di Windows — belum terverifikasi dari Linux) / **~4,4 MiB** (4.629.616 byte stripped dynamic GNU ELF di Linux) / **~4,5 MiB** (4.756.640 byte stripped static-pie musl ELF di Linux), semuanya diukur pada build rilis branch `dev-muse` dan jauh di bawah pagu target $\le 25$ MB
- **Target Distribusi:** Linux Single Static musl Binary (TERBUKTI 2026-11-12: 4.756.640 byte = 4,54 MiB, `static-pie linked`, stripped, `ldd` = statically linked, suite 98/98 di target musl; build lokal via `musl-gcc`, CI membangunnya tiap push) & Windows Native Standalone Binary (`.exe`)
- **Status Kompilasi & Tes:** ✅ `cargo test --workspace` (98 tests lolos, 0 failed di seluruh 12 crate — diukur di Linux; Windows belum dijalankan dari VM ini)
- **Status Linter:** ✅ `cargo clippy --workspace --all-targets -- -D warnings` (0 warnings)
- **Status Formatting:** ✅ `cargo fmt --check` (Clean)

---

## 1. Ringkasan Eksekutif & Persentase Proyek

PurrCode AI dirancang sebagai agen asisten personal modular (setara **Hermes Agent** dan **Muse**) dengan kemampuan terminal coding agent kelas atas (setara **Claude Code**, **OpenAI Codex CLI**, **oh-my-pi / OMP**, dan **OpenCode**).

### Posisi Tahapan Saat Ini
Kita berada pada **tahap akhir penyelesaian Milestone v0.1 (Terminal CLI & TUI Coding Agent)** yang telah mencapai status **Core Engine Selesai & Terverifikasi (Hardened)**, dan siap memasuki **Milestone v0.2 (Telegram Channel & Gateway Daemon)**.

| Lingkup Evaluasi | Progres | Status | Catatan Teknis |
|---|---|---|---|
| **Milestone v0.1: Terminal CLI & TUI Coding Agent** | **98%** | **Core Engine Selesai & Terverifikasi (Hardened)** | Seluruh 12 crate, arsitektur multi-agent The Clowder, engine streaming SSE, fail-closed approval gate, audit log, onboard wizard, dan 13 core tools (termasuk `manage_skill` untuk self-improvement) sudah berfungsi dan tervalidasi dengan 98 unit/integration tests hermetis lintas platform. |
| **Milestone v0.2: Telegram Channel & Gateway Daemon** | **~15%** | **Desain Arsitektur & Service Unit Siap** | CLI subcommand `gateway` dan generator systemd/openrc sudah terpasang di `purrcode-service`, handler bot Telegram siap diimplementasikan. |
| **Milestone v0.3: Autonomous Intelligence & OS Sandboxing** | **10%** | **Fondasi Skill Manager & Service Siap** | Tool dynamic skill synthesis (`manage_skill`), shared concurrency mutex, shadow auditor (Tuxeshadow), dan generator service daemon sudah aktif. Background cron scheduler daemon dan isolasi sandbox OS dijadwalkan pada v0.3. |
| **Milestone v0.4: WebUI Khusus (DeepSeek Harness Style)** | **0%** | **Terencana (Planned)** | Embedded web server (Axum/Actix), web dashboard canvas, hot-reloading plugin. |
| **TOTAL KESELURUHAN VISI PROJEK (v0.1 – v0.4)** | **~38%** | **Fondasi Inti Sangat Solid** | Fondasi v0.1 adalah bagian paling berat dan kompleks (agent loop, tool execution, safety gate, multi-agent, coding ergonomics, procedural skills). |

---

## 2. Matriks Komparatif Terhadap Standar Referensi

Di bawah ini adalah evaluasi pemenuhan kapabilitas PurrCode AI terhadap referensi open-source utama yang dipelajari (`docs/00-analysis-references.md`):

| Referensi Target | Fitur Kunci yang Diadopsi PurrCode | Status di PurrCode AI | Keunggulan Inovasi PurrCode AI |
|---|---|---|---|
| **Claude Code** (Anthropic) | Format `SKILL.md` (*agentskills*), agent definition Markdown frontmatter, session rollout JSONL, supervised permissions (`deny > ask > allow`), fuzzy-whitespace `edit` fallback, grep context lines (`-C`), smart output truncation (60/40). (Catatan: panduan developer `CLAUDE.md`/`AGENTS.md` memandu agen pengembang repo, sedangkan prompt runtime PurrCode memakai `SOUL.md`/`USER.md`/`MEMORY.md`). | ✅ **Tercapai di v0.1** | Berjalan sebagai **binary tunggal ramping di Rust (~4,4 MiB ELF Linux terukur; ~3,7 MB `.exe` Windows belum terverifikasi)** tanpa dependensi Node.js / Bun / npm. Zero hardcoded model, mendukung endpoint lokal / Ollama secara native. |
| **OpenAI Codex CLI** | Confinement workspace, multi-turn tool execution loop, `kill_on_drop` process termination, fail-closed security. | ✅ **Tercapai di v0.1** | Path-confinement ketat pada tool filesystem (`read`, `write`, `edit`) dan `bash`. Sanitasi kredensial otomatis pada audit log. |
| **Hermes Agent** (Nous) & **Muse** | Persona/Soul (`SOUL.md`), User context (`USER.md`), Memori persisten (`MEMORY.md`), gate minimum context-window (64K guard), gateway daemon pattern, dynamic procedural skills (`manage_skill`). | ✅ **Tercapai di v0.1** | Memori default **100% Markdown murni** tanpa dependensi database vektor berat. Nada bicara adaptif mengikuti bahasa user (bilingual/multilingual), kode internal tetap Bahasa Inggris. |
| **oh-my-pi (OMP)** & **oh-my-opencode-slim** | Ratatui TUI differential rendering, card status tool (`[running]`, `[done]`, `[failed]`), diff preview sebelum tulis ke disk, steering queue (input interupsi saat generasi), cancel flag (`Ctrl-C`), spesialisasi subagent (The Clowder). | ✅ **Tercapai di v0.1** | **Mengatasi kelemahan terbesar OpenCode-Slim:** Pada slim, subagent hanya dicegah hilang konteks lewat *prompt discipline* yang rapuh. Di PurrCode AI, ini **ditegakkan runtime**: continuation ID stabil (`purr_<id>`), file handoff (`brief.md`, `result.md`), Meowstro **dilarang keras menulis kode** (tanpa write/edit/bash), plus **Tuxeshadow (ADR-012)** sebagai auditor independen. |
| **DeepSeek Harness** | Filosofi *everything is a plugin*, spesifikasi manifest `plugin.yaml`, lifecycle hooks (`pre_tool_use` yang bisa melakukan veto sebelum eksekusi tool). | ✅ **Tercapai di v0.1** | Default-deny secrets injection: plugin tidak pernah menerima kredensial lingkungan tanpa deklarasi manifest. (Isolasi sandbox cgroup/namespace OS dijadwalkan pada v0.3). |
| **Weilin0723/PurrCode** (v1.0.0) | Kernel-level OS sandboxing (`bwrap` di Linux, `sandbox-exec` di macOS, `cap-std`), Tree-sitter AST parsing untuk ekstraksi simbol 6 bahasa, desktop GUI IDE berbasis `egui`/`eframe`. | 🔄 **Dijadwalkan di v0.3 & v0.4** (Pola arsitektur telah dianalisis di `docs/00-analysis-references.md` §2.10) | **PurrCode AI lebih unggul dalam portabilitas & kolaborasi:** Memakai **The Clowder** (multi-agent) dengan kontinuitas ID stabil untuk mencegah context-drift; memori **100% Markdown murni** tanpa beban SQLite WAL; serta berjalan langsung sebagai in-process CLI/TUI ramping (**~3,7 MB** `.exe` Windows — belum terverifikasi) tanpa kewajiban daemon latar. |

---

## 3. Bukti Verifikasi Codebase (98 Unit/Integration Tests)

Hasil pengujian otomatis per `2026-11-12 05:02:00 +07:00` (98/98 lolos di Linux; Windows belum dijalankan dari VM ini):

1. **`purrcode_api` (4 tests):**
   - `tool::tests::test_tool_registry_register_retain_and_freeze` (Pembekuan registry tool)
   - `error::tests::test_purr_error_formatting` (Format error standard)
   - `event::tests::test_agent_event_serialization_roundtrip` (Event roundtrip JSON)
   - `event::tests::test_subagent_status_serde` (Serde status subagent)
2. **`purrcode_approval` (2 tests):**
   - `tests::test_deny_beats_ask_beats_allow_and_narrowing` (Penyempitan policy & resolusi izin)
   - `tests::test_secret_scrubbing_and_audit_rotation` (Sanitasi secret & rotasi file audit)
3. **`purrcode_channel` (9 tests):**
   - `tui::app::tests::test_tui_app_utf8_editing_and_events` (Input TUI UTF-8 & penanganan event)
   - `tui::app::tests::test_tui_app_approval_flow` (Alur interaktif dialog persetujuan TUI)
   - `tui::app::tests::test_tui_app_ask_prompt_flow` (Alur interaktif dialog ask prompt TUI)
   - `tui::app::tests::test_tui_app_cursor_movement_and_editing` (Pergerakan kursor & edit input)
   - `tui::tests::test_approval_key_action_approve_keys` (Pemetaan tombol approve `a`/`y`/`Enter` di kartu approval)
   - `tui::tests::test_approval_key_action_reject_keys` (Pemetaan tombol reject `t`/`n`/`Esc` di kartu approval)
   - `tui::tests::test_approval_key_action_edit_keys` (Pemetaan tombol edit `e` di kartu approval)
   - `tui::tests::test_approval_key_action_other_keys_ignored` (Tombol lain diabaikan, kartu tetap terbuka)
   - `tui::render::tests::test_tool_result_card_shows_first_lines_and_more_marker` (Kartu hasil tool TUI: 4 baris pertama + marker `… (N more lines)`, via TestBackend)
4. **`purrcode_ai` (CLI binary) (8 tests):**
   - `onboard::tests::test_config_serialization_escaping` (Escape serialisasi config JSONC)
   - `onboard::tests::test_save_credential_safe` (Penyimpanan credentials mode 0600)
   - `onboard::tests::test_validate_endpoint_connectivity_rejects_dead_endpoint` (Validasi konektivitas HTTP & 4xx/5xx guard)
   - `tests::test_resolve_target_model_deterministic_priority` (Resolusi deterministik prioritas model sesi)
   - `tests::test_resolve_target_model_deterministic_lexicographical_fallback` (Resolusi deterministik urutan leksikografis fallback model sesi)
   - `cli::tests::test_use_orchestrator_defaults_to_orchestrator` (Default orchestrator; `--direct` opt-out; `--orchestrator` menang bila keduanya dikasih)
   - `cli::tests::test_agent_role_for_is_honest` (Label audit jujur: `meowstro` vs `agent`)
   - `tests::test_cli_approval_requester_denies_without_tty` (Requester non-TTY menolak di kedua varian `yolo`; hint `yolo` tidak lagi menyesatkan)
5. **`purrcode_config` (9 tests):**
   - `jsonc::tests::test_clean_jsonc_comments`, `jsonc::tests::test_strip_trailing_commas`
   - `loader::tests::test_expand_env_vars`, `loader::tests::test_parse_example_file`, `loader::tests::test_hierarchical_merge`
   - `validator::tests::test_rejects_model_under_64k`
   - `preset_mode::tests::test_bundled_mode_presets_load`, `preset_model::tests::test_bundled_model_presets_load`
   - `model::tests::test_resolve_model_bare_name_is_deterministic_across_providers` (Resolusi deterministik nama model telanjang antar-provider)
6. **`purrcode_core` (4 tests):**
   - `loop_engine::tests::test_policy_gate_deny_blocks_tool_and_audits`
   - `loop_engine::tests::test_plugin_pre_tool_use_veto_blocks_execution`
   - `loop_engine::tests::test_dry_run_diff_preview_and_audit_sink`
   - `session::tests::test_session_append_fork_and_compact`
7. **`purrcode_memory` (3 tests):**
   - `tests::test_markdown_memory_remember_recall_and_forget`
   - `tests::test_markdown_memory_load_context_sections` (Integrasi SOUL/USER/MEMORY/.purrcode)
   - `tests::test_truncate_chars_utf8_safety` (Pemotongan batas karakter UTF-8 aman)
8. **`purrcode_orchestrator` (8 tests):**
   - `shadow::tests::test_parse_shadow_verdicts_and_triggers`
   - `shadow::tests::test_shadow_enforcement_across_modes`
   - `shadow::tests::test_strict_shadow_user_approval_with_preview` (Verifikasi penyerahan diff preview saat persetujuan Carrier)
   - `task::tests::test_per_role_model_preset_resolution_and_precedence`
   - `task::tests::test_subagent_chain_handoff_and_resume_by_id`
   - `task::tests::test_format_validated_result_markdown_rejects_empty_and_normalizes` (Penolakan ringkasan subagent kosong & normalisasi)
   - `task::tests::test_format_validated_result_markdown_dedups_summary_heading` (Heading `## Summary` ganda di `result.md` dihapus)
   - `task::tests::test_task_tool_dry_run_returns_preview_without_side_effects` (Pencegahan eksekusi ganda subagent saat dry-run)
9. **`purrcode_plugin` (6 tests):**
   - `tests::test_parse_plugin_yaml_and_default_deny_secrets`
   - `tests::test_parse_plugin_yaml_with_declared_secrets`
   - `tests::test_parse_plugin_yaml_missing_name_rejected`
   - `tests::test_parse_plugin_yaml_records_network_and_filesystem` (Deklarasi `network`/`filesystem` di-parse & dicatat, tidak ditegakkan di v0.1)
   - `tests::test_unenforced_capabilities_reports_declared_only` (Helper `unenforced_capabilities` hanya melaporkan yang dideklarasikan)
   - `tests::test_plugin_manager_dispatch_pre_tool_use_veto`
10. **`purrcode_provider` (7 tests):**
    - `router::tests::test_policy_refusal_stops_fallback_chain`
    - `router::tests::test_technical_error_fails_over_to_next_candidate_and_cooldown` (Failover otomatis saat error teknis & cooldown)
    - `openai::tests::test_openai_provider_endpoint_url_resolution`
    - `openai::tests::test_sse_chunk_deserialization_reasoning_variants` (DeepSeek R1 / Ollama)
    - `openai::tests::test_sse_chunk_deserialization_tool_call_delta`
    - `openai::tests::test_sse_tool_call_delta_without_index_defaults_to_zero` (Chunk tool-call tanpa `index` tetap di-parse sebagai tool call pertama, bukan dibuang diam-diam)
    - `openai::tests::test_user_agent_resolution_default_and_custom` (Default UA jujur `PurrCode-AI/0.1` vs UA kustom dari config; UA invalid → kembali ke default, bukan client tanpa UA)
11. **`purrcode_service` (4 tests):**
    - `tests::test_service_unit_generators`
    - `tests::test_systemd_unit_dependencies_and_restart_policy`
    - `tests::test_openrc_init_shebang_and_network_dependency`
    - `tests::test_windows_task_script_generator` (Generator skrip PowerShell Task Scheduler Windows)
12. **`purrcode_tools` (34 tests):**
    - `execution::bash::tests::test_smart_truncate_output`
    - `execution::bash::tests::test_resolve_windows_shell_detects_git_bash_candidate` (Deteksi otomatis Git Bash di Windows)
    - `execution::bash::tests::test_resolve_windows_shell_detects_path_bash` (Deteksi bash.exe di PATH lingkungan lintas platform)
    - `execution::bash::tests::test_windows_join_uses_backslash_for_windows_paths` (Join path Windows memakai `\` di semua platform)
    - `execution::bash::tests::test_windows_join_keeps_host_separator_for_unix_paths` (Join path Unix tetap memakai separator host)
    - `execution::bash::tests::test_resolve_windows_shell_falls_back_to_powershell` (Fallback aman ke powershell.exe)
    - `fs::read::tests::test_strip_verbatim_prefix_and_confinement_windows` (Normalisasi prefix verbatim `\\?\` dan case-insensitivity Windows)
    - `web::webfetch::tests::test_utf8_safe_truncation`
    - `web::webfetch::tests::test_ssrf_blocking`
    - `web::webfetch::tests::test_resolve_redirect` (Join Location redirect absolut/relatif)
    - `web::webfetch::tests::test_resolve_validated_addrs_ip_literal` (Resolve + validasi IP literal: loopback/benchmarking ditolak, publik lolos)
    - `web::webfetch::tests::test_dns_lookup_fail_closed` (DNS gagal → fetch ditolak, bukan dilanjutkan tanpa verifikasi)
    - `web::webfetch::tests::test_get_pinned_rejects_private_destination` (Fetch ter-pin menolak target loopback sebelum socket dibuka)
    - `web::webfetch::tests::test_redirect_hop_to_loopback_is_rejected` (Hop redirect ke loopback/metadata ditolak sebelum koneksi dibuka)
    - `web::websearch::tests::test_extract_snippets_and_encode`
    - `web::websearch::tests::test_websearch_propagates_network_error`
    - `coordination::ask::tests::test_ask_empty_options`
    - `coordination::ask::tests::test_ask_non_interactive`
    - `coordination::todo::tests::test_todo_tool`
    - `skill::tests::test_embedded_skills_always_available`
    - `skill::tests::test_create_and_read_custom_skill`
    - `skill::tests::test_patch_skill_fuzzy_matching`
    - `skill::tests::test_delete_custom_skill_and_embedded_protection`
    - `skill::tests::test_skill_name_and_description_validation`
    - `skill::manage::tests::test_manage_skill_lifecycle_create_patch_list_delete`
    - `skill::manage::tests::test_manage_skill_dry_run`
    - `fs::read::tests::test_read_confinement_blocks_traversal`
    - `fs::read::tests::test_read_tool`
    - `fs::edit::tests::test_edit_unique_match_and_hash_anchor`
    - `fs::edit::tests::test_edit_fuzzy_whitespace_and_crlf_fallback`
    - `fs::edit::tests::test_find_match_range_rejects_ambiguity`
    - `fs::edit::tests::test_fuzzy_match_range_keeps_trailing_newline` (Range fuzzy berakhir sebelum newline baris terakhir; baris berikut tidak menempel)
    - `search::glob::tests::test_glob_to_regex`
    - `search::grep::tests::test_grep_basic_and_context_lines`

---

## 4. Evaluasi Penutupan Temuan Audit & Inovasi Baru

Semua temuan dari verifikasi independen Muse Code (`purrcode-ai-verifikasi-070d6f7.md`, `purrcode-ai-verifikasi-37b9b8a.md`, `purrcode-ai-verifikasi-55f1eda.md`, dan `purrcode-ai-verifikasi-26e2061.md`) dan gap analysis internal telah ditangani secara menyeluruh:

1. **Onboard API Key Validation:**
   - Menolak kode HTTP 401 Unauthorized / HTTP 403 Forbidden secara eksplisit pada probe request sebelum menyimpan konfigurasi.
2. **SSRF Websearch Destination Validation:**
   - `websearch` sekarang memvalidasi skema (hanya http/https), domain terlarang (`localhost`), alamat IP privat/loopback/metadata (termasuk Carrier-Grade NAT `100.64.0.0/10` dan Benchmarking `198.18.0.0/15`), serta pengecekan DNS asinkron untuk mitigasi DNS-rebinding.
3. **Workspace Directory CLI Override:**
   - Menambahkan flag `-w, --workspace <path>` pada CLI parser dan menyalurkannya ke seluruh TUI, One-Shot, dan subcommands.
4. **Subagent Continuation ID Persistence:**
   - `SubagentRegistry::get_or_create` mengembalikan `Result<String>` dan mempropagasi kegagalan penulisan disk (`?`), tidak ada error yang di-swallow.
5. **Coding Ergonomics (Claude Code & Codex Parity):**
   - Tool `edit`: Fallback normalisasi whitespace & CRLF saat pencarian teks eksak meleset karena format model LLM.
   - Tool `grep`: Menambahkan parameter `context` (`-C`) dengan format standar diff (`:` untuk match, `-` untuk konteks).
   - Tool `bash`: Menambahkan pemotongan cerdas (*smart truncation*: 60% head + 40% tail) untuk melindungi window konteks.
6. **Self-Improvement / Procedural Skill Management (`manage_skill`):**
   - Mengadopsi arsitektur Procedural Memory dari Hermes Agent dan review otomatis dari Muse AI.
   - Subagent Tinker dapat membuat skill baru (`create`), menambal instruksi skill yang sudah ada (`patch` dengan preview unified diff), menghapus skill override dan me-restore fallback embedded (`delete`), serta melihat katalog lengkap (`list`).
   - Meowstro bertindak sebagai orchestrator read-only yang menginjeksi daftar skill ringkas ke prompt sistem dan membaca detail instruksi via `read_skill`.
7. **Kedalaman Unit Tests di Seluruh Crate:**
   - Setiap crate di workspace kini memiliki unit tests komprehensif (total 90 tests saat commit e7c556b, 100% lolos di Linux; Windows belum dijalankan dari VM ini; angka terbaru lihat header dokumen dan item 15).
8. **Verifikasi Lanjutan Commit 37b9b8a (Muse Spark Audit Resolution):**
   - **Hermetisitas Pengujian SSRF:** Menghapus resolusi DNS soket sinkron (`to_socket_addrs`) dari `validate_destination_for_tool` yang sebelumnya memicu false positive di lingkungan dengan reverse/transparent proxy (seperti isolasi VM Muse yang memetakan domain publik ke range `198.18.0.0/15`). Validasi DNS asinkron tetap aktif di runtime via `validate_dns_lookup_async_for_tool`, sementara unit test `test_ssrf_blocking` kini menguji vektor IP dan DNS-rebinding secara offline dan 100% deterministik.
   - **Penyaluran `workspace_root` Menyeluruh:** Menyalurkan parameter `workspace_root` ke `ToolRegistryExt::with_defaults_and_options`, `MeowstroOrchestrator::with_complete_options`, dan `run_one_shot`, sehingga skill projek di `<workspace_root>/.purrcode/skills/` terdeteksi secara konsisten saat flag `-w, --workspace` digunakan.
   - **Normalisasi Schema `result.md`:** Menegaskan bahwa validasi `result.md` menggunakan pendekatan normalisasi struktural untuk menyisipkan header standar (`Status`, `Role`, `Timestamp`, `Changed Files`, `Verified Evidence`, `Failures`, `Unverified Items`) jika LLM subagent melewatkannya, sembari menolak keras laporan yang kosong atau hanya whitespace.
   - **Klarifikasi Roadmap v0.1 vs v0.3:** Menegaskan batasan ruang lingkup bahwa daemon scheduler cron background dan alur konfirmasi interaktif instalasi plugin dirancang untuk v0.3, sementara generator service unit systemd/openrc dan validasi manifes izin plugin sudah aktif dan teruji di v0.1.
9. **Verifikasi Live PTY Commit 55f1eda (Muse.ai Interactive Audit Resolution):**
   - **Pencegahan Eksekusi Ganda Subagent pada Dry-Run (`TaskTool`):** Memeriksa `ctx.is_dry_run` di awal `TaskTool::execute`. Jika true, langsung mengembalikan rencana delegasi (peran, model, ID, objektif, dan plan/diff) sebagai `diff_preview` tanpa mengalokasikan ID di registry, tanpa menyentuh disk, dan tanpa memanggil child loop. Menghilangkan bug eksekusi ganda yang ditemukan pada pengujian interaktif PTY di mode Carrier/Strict.
   - **Pencatatan Status Gagal (`failed`) di Registry:** Jika laporan subagent kosong atau gagal ditulis ke `result.md`, `TaskTool` memanggil `reg.record_failure` sebelum me-return error, sehingga status subagent di disk tersimpan sebagai `"failed"` (bukan tertinggal `"running"`).
   - **Penyaluran `workspace_root` ke Toolset Subagent Child:** `build_role_tools` meneruskan `Some(&ctx.workspace_root)` ke `ToolRegistry::with_defaults_and_options`, sehingga subagent Tinker dapat menemukan skill projek di `<workspace_root>/.purrcode/skills/` saat flag `-w` digunakan.
   - **Pengukuran Ukuran Binary Release Riil:** Kompilasi rilis `cargo build --release` menghasilkan binary ramping berukuran **~3,7 MB** (`.exe` di Windows — belum terverifikasi dari Linux) dan **~4,4 MiB** (stripped dynamic ELF di Linux; angka byte eksak per build tercatat di header dokumen dan §4 item 12/13), membuktikan terpenuhinya target non-fungsional PRD $\le 25$ MB.
10. **Windows Non-WSL Architecture (Paritas Claude Code & Codex CLI):**
    - **Smart Shell Detector (`crates/purrcode-tools/src/execution/bash.rs`):** Deteksi otomatis instalasi Git Bash di Windows (`C:\Program Files\Git\bin\bash.exe`, dsb., serta `bash.exe` di `%PATH%`). Menjalankan perintah dengan flag `-c` untuk memastikan kompatibilitas penuh skrip POSIX (piping, `grep`, forward-slash paths) dengan fallback transparan ke `powershell.exe -NoProfile -NonInteractive -Command` jika Git Bash tidak ditemukan. Metadata tool mencatat runtime shell aktif (`"shell": "git-bash"` atau `"shell": "powershell"`).
    - **Normalisasi Prefix Verbatim Windows `\\?\` (`crates/purrcode-tools/src/fs/read.rs`):** Menghapus prefix verbatim `\\?\` dan `\\?\UNC\` yang dihasilkan oleh `std::fs::canonicalize` di Windows via `strip_verbatim_prefix`, serta menerapkan pembandingan path case-insensitive via `path_starts_with_confinement`. Mengeliminasi bug di mana path absolut Windows (misal `F:\project\...`) dianggap melanggar *workspace confinement* terhadap `\\?\F:\project\...`.
    - **Windows Task Scheduler Service Installer (`crates/purrcode-service/src/lib.rs`):** Menambahkan fungsi `generate_windows_task_script` yang menghasilkan skrip otomatisasi PowerShell untuk mendaftarkan PurrCode AI gateway daemon sebagai *Scheduled Task* Windows (`Register-ScheduledTask`) dengan trigger `@AtLogOn` dan restart otomatis jika gagal. Subcommand `purrcode-ai service install` di Windows otomatis menghasilkan skrip ini.
    - **Guard Codepage UTF-8 Konsol Windows (`crates/purrcode-cli/src/main.rs`):** Menginisialisasi konsol Windows ke codepage 65001 (`SetConsoleOutputCP(65001)` dan `SetConsoleCP(65001)`) pada saat startup, menjamin rendering Unicode, emoji, dan karakter grafis kotak TUI tampil jernih tanpa mojibake di CMD / PowerShell konvensional.
11. **Resolusi Verifikasi Lanjutan Commit 26e2061 (Muse Spark Linux VM Audit Resolution):**
    - **Cross-Platform PATH Splitter di Unit Test (`crates/purrcode-tools/src/execution/bash.rs`):** Commit `c56d3b7` memperbaiki pembacaan PATH mock Windows agar memecah `;` secara toleran — tetapi itu TIDAK menyelesaikan regresi test `test_resolve_windows_shell_detects_path_bash` di Linux: `PathBuf::join("bash.exe")` di Linux menyisipkan `/` sehingga kandidat `C:\msys64\usr\bin/bash.exe` tidak cocok dengan path yang diharapkan. Perbaikan sesungguhnya dilakukan di branch `dev-muse` via helper `windows_join` (membangun path bergaya Windows dengan pemisah `\` eksplisit di semua platform) + 2 unit test baru; suite kini 90/90 hijau di Linux. Klaim "76/76 hijau di Linux dan Windows" dihapus sampai ada bukti uji Windows sungguhan.
    - **Deterministik Resolusi Model Sesi (`crates/purrcode-cli/src/main.rs`):** Mengurutkan daftar provider dan model secara leksikografis serta memprioritaskan model dengan penamaan `primary`, `default`, atau `main`. Mengeliminasi bug nondeterministik di mana iterasi acak `HashMap::keys().next()` memilih model sesi yang berbeda-beda setiap kali binary dijalankan tanpa flag `-m`.
    - **Penyempurnaan Ergonomi Dialog Approval TUI (`crates/purrcode-channel`):** Menambahkan dukungan tombol `y`/`Y`/`Enter` untuk menyetujui aksi (melengkapi tombol `a`/`A`) dan `n`/`N` untuk menolak (melengkapi `t`/`T`/`Esc`). Menyesuaikan tinggi kontainer card approval dari 8 ke 10 baris agar baris petunjuk aksi `[a]/[y] Approve / [t]/[n] Reject / [e] Edit` tidak terpotong saat preview diff ditampilkan.
12. **Resolusi Verifikasi Lanjutan Commit c56d3b7 (branch `dev-muse`, Skyla AI sebagai implementor):**
    - **Test PATH Windows benar-benar portabel (`crates/purrcode-tools/src/execution/bash.rs`):** Helper baru `windows_join` membangun path kandidat Windows dengan pemisah `\` eksplisit (bukan `PathBuf::join` yang memakai separator host) + 2 unit test baru (`test_windows_join_uses_backslash_for_windows_paths`, `test_windows_join_keeps_host_separator_for_unix_paths`). `test_resolve_windows_shell_detects_path_bash` kini lolos di Linux.
    - **Deterministik langkah-3 `resolve_model` (`crates/purrcode-config/src/model.rs`):** Pencarian nama model telanjang kini mengiterasi provider dalam urutan leksikografis (bukan `HashMap`), + unit test `test_resolve_model_bare_name_is_deterministic_across_providers` (20 iterasi, dua provider berbagi key yang sama → selalu provider leksikografis terkecil).
    - **Pemetaan tombol approval TUI teruji (`crates/purrcode-channel/src/tui/mod.rs`):** Match inline di `run_loop` diekstrak ke fungsi murni `approval_key_action` (enum `ApprovalKeyAction`: Approve / Reject / CancelForEdit) + 4 unit test baru. Perilaku live tidak berubah (`a`/`y`/`Enter` approve, `t`/`n`/`Esc` reject, `e` edit, tombol lain diabaikan).
    - **Kejujuran dokumen:** Semua klaim "76 lolos, 0 failed" / "100% passing di Windows & Linux" / "76/76 hijau" diganti angka hasil ukur sendiri (83/83 di Linux saat commit cde37f9; angka terbaru lihat header dokumen dan item 14); "3.7 MB" telanjang dikualifikasi sebagai `.exe` Windows yang belum terverifikasi; angka byte eksak diperbarui ke build rilis branch ini saat itu (4.617.256 byte = 4,40 MiB; angka terbaru lihat header dokumen dan item 14); "7 bahasa" Weilin0723 di §2 dikoreksi ke 6; "Bulletproof" dihapus; `AGENTS.md`/`CLAUDE.md` tidak lagi menyatakan musl sebagai fakta (musl tetap target distribusi); klaim "user approved it at install" di `AGENTS.md` dikoreksi ke scope v0.3; "~140 MB" Weilin0723 diganti "tidak terverifikasi".
    - **Pembersihan:** Direktori root `src/` yang kosong (0 entri sejak 10 Nov) dihapus.
    - **Hasil ukur saat commit 7c64b2e:** `cargo check` lolos 0 warning, `cargo test --workspace` **83/83 lolos, 0 failed**, `cargo clippy --workspace --all-targets -- -D warnings` lolos, `cargo fmt --check` bersih, `cargo build --release` → 4.617.256 byte (4,40 MiB), ELF dynamic GNU, stripped. Angka terbaru lihat header dokumen dan item 14.
13. **Provider Dev-Testing Magpie (`purrcode.jsonc.example`, `model-presets/magpie.jsonc`, `crates/purrcode-config/src/preset_model.rs`):**
    - Provider `magpie` (custom OpenAI-compatible, `https://magpie.raevtar.dev/v1`, `"api_key": "${MAGPIE_API_KEY}"` dari env saja) + preset `magpie` (semua role → `magpie:antigravity/gemini-3.8-flash-tiered`) + embedded di `preset_model.rs` + assert di `test_bundled_model_presets_load`. Test suite tetap 83/83 saat commit 7c64b2e (angka terbaru lihat header dokumen dan item 15).
    - Terverifikasi live dari VM ini (2026-11-12): `/v1/models` 200 (66 model), completion 200 ("OK", 7,8 dtk), tool call 200 (`calc` a=2 b=2, 20,5 dtk); binary `-p` baca file via model asli (25,5 dtk); delegasi `--orchestrator --yolo` end-to-end (Meowstro `task` → Tinker `write` + `read` + `bash od` verifikasi, ~157 dtk, file berisi tepat "hi from magpie").
    - **Keterbatasan lingkungan (bukan bug wiring):** binary membuat HTTPS langsung; VM ini hanya bisa keluar lewat proxy egress → pengujian dari VM memakai bridge lokal `127.0.0.1:3457` (passthrough, `/tmp/magpie_bridge.py`, bukan bagian repo). Di PC user binary langsung nyambung.
    - **Temuan baru (terbuka):** (a) User-Agent default `reqwest` di-block Cloudflare gateway ini (HTTP 403, error 1010 "browser signature") — curl/Python lolos; bridge mengirim UA browser agar tes jalan. Perlu allowlist di Cloudflare atau opsi custom User-Agent di provider. **Koreksi 2026-11-12:** binary lama ternyata TIDAK mengirim header User-Agent sama sekali — reqwest 0.12 `Client::new()` tidak menyuntik UA — dan WAF kemungkinan menolak request tanpa UA itu, bukan literal `reqwest/*`; default jujur PurrCode lolos live (Tes D). (b) **`purrcode-ai -p` tanpa `--orchestrator` TIDAK menjalankan Meowstro** — yang jalan agent generik dengan semua tool (preset `tabby`/`carrier`/`oyen` semua mencantumkan `write`/`edit`/`bash`), tapi audit melabelinya "meowstro" (`main.rs:816`, `loop_runner` pakai `m.agent_loop()` hanya bila `--orchestrator`). Jalur read-only Meowstro (`meowstro.rs:116-155`) hanya aktif dengan flag itu. Verifikasi ronde lalu yang memakai `-p` tanpa flag ini tidak menguji jalur orchestrator — pengujian delegasi wajib memakai `--orchestrator`.
14. **Ronde autonomous 2026-11-12: `--orchestrator` default, User-Agent configurable, TOCTOU SSRF tertutup (PRD §4.5.6, F10, F12):**
    - **`--orchestrator` default (PRD §4.5.6):** `cli::use_orchestrator(orchestrator, direct)` — orchestrator default untuk `-p`, `exec`, dan TUI; `--direct` opt-out ke agent generik semua-tool; `--orchestrator` eksplisit menang bila keduanya dikasih. Audit kini melabeli jujur (`cli::agent_role_for`: `"meowstro"` vs `"agent"`). Diuji unit (`test_use_orchestrator_defaults_to_orchestrator`, `test_agent_role_for_is_honest`) + live via Magpie (Tes E: `-p` polos → Meowstro `task` → Tinker `write` + verifikasi; Tes F: `--direct` → `agent tool_call write` langsung).
    - **User-Agent (F12):** `ProviderConfig.user_agent` (opsional) + default jujur `PurrCode-AI/0.1 (purrcode-ai; https://github.com/latifangren/PurrCode-AI)` di `OpenAiCompatibleProvider` (client builder `.user_agent(...)`; bila UA tidak valid, builder gagal dan fallback memakai client polos — celah ini ditemukan verifikasi independen, ditutup batch 2a via validasi (item 15), lalu fallback-nya dihapus sepenuhnya (item 16): kegagalan builder kini error di startup). Diuji unit (`test_user_agent_resolution_default_and_custom`) + live via bridge pass-through `127.0.0.1:3458`: UA Chrome config → 200, **default PurrCode UA → 200 juga** (kontrol negatif gagal dengan hasil terbaik: yang diblokir Cloudflare ternyata request TANPA header User-Agent sama sekali — binary lama tidak mengirim UA — bukan literal `reqwest/*`) — provider `magpie` di example TIDAK memakai UA browser.
    - **TOCTOU SSRF tertutup (F10):** `webfetch.rs` — `resolve_validated_addrs` (resolve sekali → validasi semua IP → fail-closed bila DNS gagal) + `build_pinned_client` (reqwest `.resolve_to_addrs`, terverifikasi dari source reqwest 0.12.28) + `get_pinned` (redirect manual max 5 hop, re-validasi + re-pin tiap hop). `websearch` memakai helper yang sama. Set IP yang diblokir tidak berubah. Pola dari zeroclaw (`resolve_to_addrs`, per-hop re-validation).
    - **Riset referensi:** subagent read-only atas clone lokal (codex, pi, opencode, picoclaw, zeroclaw, hermes, dll) — Q1 (UA: pola pi/codex + override picoclaw), Q2 (norma industri = single agent default; PurrCode sengaja menyimpang atas keputusan eksplisit user + PRD yang mengikat), Q3 (pinning zeroclaw), Q4 (egress allowlist zeroclaw + Rule opencode, untuk item 18).
    - **Hasil ukur batch ini:** `cargo check` 0 warning, `cargo test --workspace` **90/90 lolos, 0 failed**, clippy `-D warnings` lolos, fmt bersih, `cargo build --release` → 4.623.400 byte (4,41 MiB), ELF dynamic GNU, stripped.
15. **Ronde autonomous batch 2a 2026-11-12: temuan minor N1/N2/R1/R2 + capabilities jujur + light fixes (workplan item 18, 19):**
    - **N1 — UA invalid tidak lagi menghasilkan request tanpa UA (`crates/purrcode-provider/src/openai.rs`):** nilai `user_agent` dari config divalidasi dengan `HeaderValue::from_str` sebelum membangun client; bila invalid → diganti `DEFAULT_USER_AGENT` (field `user_agent` yang tersimpan selalu sama dengan yang benar-benar dikirim). Unit test diperluas dengan kasus UA berisi karakter kontrol (`"Bad\u{1}UA"` → default).
    - **N2 — `build_pinned_client` fail-closed (`crates/purrcode-tools/src/web/webfetch.rs`):** mengembalikan `Result<reqwest::Client>`; kegagalan builder kini error, bukan fallback ke client polos tanpa pinning (yang akan membatalkan jaminan pinning + redirect manual). `get_pinned` mempropagasi error dengan `?`.
    - **R2 — bukti hop redirect (`webfetch.rs`):** helper baru `next_hop_url` (join `Location` + validasi target hop SEBELUM diikuti; iterasi berikutnya re-resolve + re-pin DNS-nya) dipakai `get_pinned`; `resolve_redirect` kini menerima `tool_name` agar pesan error-nya benar untuk `websearch` juga. Unit test `test_redirect_hop_to_loopback_is_rejected` membuktikan hop ke loopback/`localhost`/metadata/FTP ditolak dan hop ke host publik lolos pemeriksaan hop.
    - **R1 — angka usang:** tabel §1 "83 unit/integration tests" dikoreksi ke 96 (sisa global replace 83→90 dari batch sebelumnya).
    - **Capabilities jujur (item 18, sisi "hapus klaimnya"):** hanya `secrets` yang ditegakkan di v0.1 — `network`/`filesystem` kini di-parse dari `plugin.yaml` (sebelumnya diam-diam dibuang parser) dan dicatat sebagai deklarasi untuk sandbox OS v0.3. `ProcessPlugin::new` memberi `tracing::warn!`; `PluginManager::unenforced_capabilities()` melaporkan plugin yang mendeklarasikannya; CLI mencatat satu audit entry `plugin_capability_unenforced` (decision `warn`) per plugin di jalur `-p` dan TUI, dan `purrcode-ai plugin list` menampilkan baris peringatan `⚠ ... NOT enforced in v0.1`. Klaim "default-deny" di `AGENTS.md`, `CLAUDE.md`, dan `ARCHITECTURE.md` §5 + doc comment `PluginCapabilities` dikualifikasi ke `secrets` saja. Penegakan nyata tetap butuh sandbox OS (v0.3, PRD F21).
    - **Light fixes (item 19):** (a) `format_validated_result_markdown` (`task.rs`) membuang heading `## Summary` di awal ringkasan child sebelum menambahkan heading kanonikal — `result.md` kini berisi heading itu tepat 1x (unit test `test_format_validated_result_markdown_dedups_summary_heading`). (b) `find_match_range` (`fs/edit.rs`) — range fuzzy kini berakhir SEBELUM newline baris terakhir yang cocok (helper `line_content_end`), jadi baris berikut tidak lagi menempel ke baris terakhir replacement; berlaku untuk LF dan CRLF (unit test `test_fuzzy_match_range_keeps_trailing_newline`, multi-line agar benar-benar menempuh jalur fuzzy, bukan exact-match substring). (c) Kartu hasil tool TUI (`render.rs`) menampilkan 4 baris pertama hasil + marker `… (N more lines)` bila terpotong, bukan hanya baris pertama; diuji unit via TestBackend (`test_tool_result_card_shows_first_lines_and_more_marker`).
    - **Koreksi rumusan UA di docs:** `purrcode.jsonc.example` (komentar magpie), workplan item 16, AUDIT_STATUS item 13(a) dan item 14 — "yang diblokir hanya UA `reqwest/*`" diganti rumusan yang benar: binary lama tidak mengirim header User-Agent sama sekali; WAF kemungkinan menolak request tanpa UA itu.
    - **Smoke test live (sendiri, sebelum commit):** plugin `net-plug` (deklarasi network + filesystem) di workspace uji → `purrcode-ai plugin list` menampilkan baris `⚠`; run `-p` via mock SSE lokal → audit log berisi tepat 1 entry `plugin_capability_unenforced`.
    - **Hasil ukur batch ini:** `cargo check` 0 warning, `cargo test --workspace` **96/96 lolos, 0 failed** (90 + 6 baru: redirect-hop, fuzzy-newline, dedup Summary, 2 plugin, kartu TUI), clippy `-D warnings` lolos, fmt bersih, `cargo build --release` → 4.629.032 byte (4,41 MiB), ELF dynamic GNU, stripped.
16. **Ronde autonomous batch 2b 2026-11-12: temuan checker batch 2a (F1/F2/N1b) + SSE parser tidak lagi diam-diam membuang chunk (workplan item 21):**
    - **F1 — angka usang §3:** header `purrcode_orchestrator` di §3 tertulis "(7 tests)" padahal daftarnya 8 nama (test dedup ditambahkan batch 2a tanpa menaikkan header) — dikoreksi ke 8.
    - **F2 — hint `--yolo` menyesatkan (`crates/purrcode-cli/src/main.rs`):** `CliApprovalRequester` kini membawa field `yolo`; saat stdin bukan TTY dan `--yolo` SUDAH dipakai, pesannya jujur — penolakan berasal dari aturan policy yang tidak bisa di-override `--yolo` (mis. write di luar workspace), bukan dari flag yang kurang. Unit test `test_cli_approval_requester_denies_without_tty` membuktikan kedua varian menolak di non-TTY.
    - **N1b — fallback `Client::new()` dihapus (`crates/purrcode-provider/src/openai.rs`, `lib.rs`):** fallback lama `.unwrap_or_else(|_| reqwest::Client::new())` bukan degradasi yang aman — dari source reqwest 0.12.28, `Client::new()` adalah `build().expect("Client::new()")`, jadi kegagalan builder karena alasan bukan-UA akan PANIC, bukan berjalan tanpa UA. `OpenAiCompatibleProvider::new` kini mengembalikan `Result` (gagal membangun client HTTP = `PurrError::Provider`, error startup yang jelas) dan `ProviderRegistry::from_config` mengembalikan `Result` — 2 call-site produksi memakai `?`, `run_doctor` melaporkan kegagalan sebagai diagnosis (bukan crash). Klaim "tidak pernah tanpa UA" kini benar secara harfiah: tidak ada lagi `Client::new()` di jalur provider.
    - **SSE parser — chunk malformed tidak lagi diam-diam dibuang (`openai.rs`):** (a) `SseToolCallDelta.index` kini `#[serde(default)]` — gateway yang menghilangkan `index` pada tool-call delta tetap di-parse sebagai tool call pertama (index 0) alih-alih menggagalkan seluruh chunk (unit test `test_sse_tool_call_delta_without_index_defaults_to_zero`). (b) Chunk yang tetap gagal di-parse kini dicatat `tracing::warn!` dengan preview 200 karakter — sebelumnya `if let Ok(parsed)` membuangnya tanpa jejak, sehingga tool call yang hilang terlihat persis seperti model yang "tidak berkata apa-apa".
    - **TERBUKA — F3 (failover top-level belum ada, klarifikasi re-checker):** `FallbackRouter` hanya dibangun di loop child (`task.rs:312`, dari `fallback_candidates` preset model per-role); panggilan pertama model sesi yang 500 → hard error by design, bukan failover. Menutup ini butuh keputusan desain — dari mana rantai fallback model sesi berasal (tidak ada field `fallback` di `ModelConfig` saat ini) — dicatat sebagai item kerja terpisah, bukan diklaim tertutup di sini.
    - **Hasil ukur batch ini:** `cargo check` 0 warning, `cargo test --workspace` **98/98 lolos, 0 failed** (96 + 2 baru: SSE index-default, CLI approval requester), clippy `-D warnings` lolos, fmt bersih, `cargo build --release` → 4.629.616 byte (4,41 MiB), ELF dynamic GNU, stripped.
    - **Catatan proses (perintah user 2026-11-12 04:54 WIB):** batch ini TIDAK disertai laporan verifikasi gabungan — kedua checker batch 2a sudah menutup klaim batch 2a (6/6 build + re-check 96/96), dan temuan mereka (F1/F2/N1b + observasi SSE) langsung menjadi daftar perbaikan ini. Verifikasi independen per batch tetap jalan diam-diam; laporan hanya di ujung v0.1.
17. **Musl statis + CI (workplan item 20, 2026-11-12):**
    - **Build musl sungguhan (bukan klaim):** `cargo build --release --target x86_64-unknown-linux-musl` dengan `CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=musl-gcc` + `CC_x86_64_unknown_linux_musl=musl-gcc` (target + `musl-tools` 1.2.4 terpasang di VM). Hasil: **4.756.640 byte = 4,54 MiB**, `file` = `ELF 64-bit LSB pie executable, x86-64, version 1 (SYSV), static-pie linked, stripped`, `ldd` = `statically linked`, `--help` berjalan. Static-PIE (bukan static biasa) karena Rust memakai relocation-model pic untuk target musl — tetap tanpa dynamic loader dan tanpa dependensi `.so` apa pun.
    - **Suite di target musl: 98/98, 0 failed** — jumlah dan per-crate sama persis dengan suite GNU (api 4, approval 2, channel 9, cli 8, config 9, core 4, memory 3, orchestrator 8, plugin 6, provider 7, service 4, tools 34). Binary musl bukan sekadar ter-link, tapi teruji.
    - **CI (`.github/workflows/ci.yml`, baru):** 6 job — `fmt`, `clippy -D warnings`, `test` (Linux GNU), `release-gnu`, `release-musl` (install `musl-tools`, build target musl, verifikasi `file` + `ldd` statically linked, upload artifact), `windows` (test + release di `windows-latest`). YAML divalidasi lokal (PyYAML, 6 job). **Jujur dicatat:** CI belum pernah berjalan di GitHub (belum ada push) — job Windows khususnya tidak terverifikasi dari VM ini dan hanya berjalan di runner GitHub; artifact upload belum teruji end-to-end. Sampai ada run hijau pertama di GitHub, CI ini adalah pipeline yang dibuat + divalidasi sintaksnya, bukan pipeline yang terbukti jalan.
    - **Batas scope:** tidak memakai `cargo zigbuild`/`cross` (tidak diperlukan — `musl-gcc` cukup); cross-compile arm64/arm/mipsle (PRD F14) tetap belum dikerjakan.

---

## 5. Rencana Aksi Strategis Berikutnya (Kickoff v0.2)

Dengan kapabilitas inti Milestone v0.1 yang telah **selesai dan terverifikasi secara mendalam**, langkah strategis berikutnya adalah memulai **Milestone v0.2: Telegram Channel & Gateway Daemon**:

1. **Implementasi Telegram Bot Handler (`crates/purrcode-channel`):**
   - Integrasikan long-polling dan webhook Telegram client (via `teloxide` atau reqwest polling ringan) agar binary tetap ramping.
   - Sambungkan user ID Telegram yang terotorisasi dengan session manager di `purrcode-core`.
2. **Gateway Daemon Service Integration:**
   - Jalankan subcommand `purrcode-ai gateway` sebagai background service melalui systemd / OpenRC unit yang sudah digenerate oleh `purrcode-service`.
3. **Multi-Channel Orchestration:**
   - Memungkinkan user mengirim tugas coding ke Meowstro lewat chat Telegram, dan Meowstro mendisposisikan subagent Tinker/Whisker sambil melaporkan status kembali ke Telegram.
