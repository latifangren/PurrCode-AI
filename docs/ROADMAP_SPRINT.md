# PurrCode AI — Panduan Eksekusi & Bukti Uji (ROADMAP_SPRINT.md)

- **Status Terakhir:** Seluruh Sprint 1 s/d 9 (Milestone v0.1) selesai dan terverifikasi secara mendalam (98 tests lolos di seluruh 12 crate — diukur di Linux, branch `dev-muse`).
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
- [x] **9.8 Ronde autonomous batch 2b 2026-11-12 (temuan checker batch 2a F1/F2/N1b + SSE parser) — hint `--yolo` jujur + konstruktor provider fallible + chunk SSE tidak lagi dibuang diam-diam (`crates/purrcode-provider/`, `crates/purrcode-cli/`, `docs/`)**
  - F1: header `purrcode_orchestrator` di §3 AUDIT_STATUS "(7 tests)" dikoreksi ke 8 (test dedup batch 2a). F2: `CliApprovalRequester` membawa field `yolo` — penolakan non-TTY saat `--yolo` sudah dipakai kini menjelaskan aturan policy yang tidak bisa di-override `--yolo` (mis. write di luar workspace); unit test `test_cli_approval_requester_denies_without_tty`.
  - N1b: fallback `.unwrap_or_else(|_| reqwest::Client::new())` dihapus — dari source reqwest 0.12.28, `Client::new()` adalah `build().expect(...)`, jadi kegagalan builder bukan-UA akan PANIC. `OpenAiCompatibleProvider::new` dan `ProviderRegistry::from_config` kini mengembalikan `Result`; call-site produksi memakai `?`, `run_doctor` melaporkan sebagai diagnosis. Tidak ada lagi `Client::new()` di jalur provider.
  - SSE: `SseToolCallDelta.index` kini `#[serde(default)]` — gateway yang menghilangkan `index` tetap di-parse sebagai tool call pertama (unit test `test_sse_tool_call_delta_without_index_defaults_to_zero`); chunk yang tetap gagal di-parse dicatat `tracing::warn!` dengan preview 200 karakter, bukan dibuang diam-diam.
  - TERBUKA (F3): failover top-level belum ada — `FallbackRouter` hanya di loop child; model sesi 500 → hard error by design. Menutupnya butuh keputusan desain sumber rantai fallback model sesi (tidak ada field `fallback` di `ModelConfig`).
  - Hasil ukur: check 0 warning, test 98/98, clippy `-D warnings` bersih, fmt bersih, release 4.629.616 byte (4,41 MiB) ELF dynamic GNU stripped.
- [x] **9.9 Musl statis + CI 2026-11-12 (workplan item 20) — binary statis musl terverifikasi + workflow GitHub Actions (`.github/workflows/ci.yml`)**
  - Build musl: `cargo build --release --target x86_64-unknown-linux-musl` (linker/CC `musl-gcc`, `musl-tools` 1.2.4) → **4.756.640 byte = 4,54 MiB**, `static-pie linked`, stripped, `ldd` = statically linked, `--help` berjalan. Suite di target musl **98/98, 0 failed** — sama persis dengan suite GNU.
  - CI 6 job: `fmt`, `clippy -D warnings`, `test` (GNU), `release-gnu`, `release-musl` (verifikasi `file` + `ldd` + upload artifact), `windows` (test + release). YAML divalidasi lokal (6 job). Jujur: CI belum pernah berjalan di GitHub (belum ada push) — job Windows tidak terverifikasi dari VM ini; sampai run hijau pertama, ini pipeline yang dibuat + divalidasi sintaksnya, bukan yang terbukti jalan. Tidak memakai zigbuild/cross; cross arm64 tetap belum dikerjakan.
- [x] **9.10 Dogfood v0.1 2026-11-12 (TUI + `-p` + `--json`, model Magpie) + perbaikan temuan D2/D3 (workplan item 23) — child cap 25 + hasil task inline + `[A] Always (session)` (`crates/purrcode-orchestrator/`, `crates/purrcode-channel/`, `crates/purrcode-cli/`, `docs/`)**
  - TUI: proyek Rust `mdcount` (scaffold + implementasi + test + README) selesai end-to-end via Meowstro → Tinker (continuation ID sama, resume); terverifikasi independen: `cargo test` 21/21, clippy 0 warning, JSON output persis (70 kata / 31 baris / 2 code block).
  - `-p --json --yolo`: flag `--words-only` benar — `cargo test` 26/26, `cargo run -- --words-only sample.md` → `70`. Run dihentikan di tengah loop pencarian toolchain (artefak harness PATH, bukan defect produk).
  - D2: kartu TUI kini `[A] Always (session)` — `ApprovalRequest.always` + `TuiApprovalRequester.session_allow` fast path; hanya melewati prompt, hard denial gate tetap berlaku. Terverifikasi live: 2 file + verifikasi xxd/od selesai dengan SATU kali tekan `A` (audit 4× `approved_by_user`, tanpa kartu kedua).
  - D3: child cap 15 → 25 langkah (task.rs:622, = default parent loop); baris hasil task tidak lagi menunjuk path `result.md` yang tidak bisa dibaca orchestrator (record-only, summary inline). Konfinemen tidak dilonggarkan.
  - TERBUKA (dicatat jujur, bukan diklaim): D5 `--json` hanya delta teks (tanpa event tool_call/session); D6 bash stateless (export tidak bertahan antar panggilan); F3 failover top-level.
  - Hasil ukur: check 0 warning, test 99/99, clippy `-D warnings` bersih, fmt bersih, release 4.630.000 byte (4,41 MiB) ELF dynamic GNU stripped.
- [x] **9.11 Batch 2d: perbaikan temuan verifikasi batch 2c — dead-model fallback + 3 koreksi dokumen (workplan item 24) — `crates/purrcode-orchestrator/`, `docs/`**
  - F1 (moderate): `resolve_provider_and_model` — bila tidak ada kandidat fallback ter-resolve, child kini jalan di provider + model SESI (`self.provider`, `self.model`), bukan id model preset (`ollama:qwen2.5-coder:7b`) yang dikirim ke provider sesi (provider ketat 404, label salah). Regression test `test_unresolved_role_model_falls_back_to_session_model` (`Arc::ptr_eq` provider sesi).
  - F1–F3 re-checker (dokumen): angka musl 98/98 → 100/100; anchor D5 main.rs:892 → :902; gloss ganda baris §3 dirapikan.
  - Hasil ukur: check 0 warning, test 100/100, clippy `-D warnings` bersih, fmt bersih, release GNU 4.630.000 byte; suite musl 100/100, rilis musl 4.756.640 byte static-pie.
  - Light fixes: dedup heading `## Summary` di `result.md`; range fuzzy `edit` berakhir sebelum newline baris terakhir (LF + CRLF); kartu hasil tool TUI menampilkan 4 baris pertama + marker `… (N more lines)` (diuji via TestBackend).
  - Koreksi rumusan UA di docs: "yang diblokir hanya UA `reqwest/*`" diganti "binary lama tidak mengirim header User-Agent sama sekali".
  - Hasil ukur: `cargo test --workspace` 96/96, `cargo build --release` 4.629.032 byte (4,41 MiB).

## 10. Milestone v0.2 — Telegram Channel & Gateway Daemon (F15–F18)

- [x] **10.1 Telegram v0.2 2026-11-12 — gateway daemon + channel Telegram hand-rolled (workplan item 25) — `crates/purrcode-channel/src/telegram/`, `crates/purrcode-cli/src/gateway.rs`, `crates/purrcode-config/`**
  - ADR-013 (diamendemen batch 2e): klien Bot API hand-rolled reqwest (tanpa teloxide), `api_base` configurable; SSRF pinning TIDAK berlaku untuk `api_base` (deviasi disengaja: tidak ada URL kendali-model, pinning merusak self-hosted Bot API di LAN + mock lokal); `allow_from` fail-closed; sesi = chat ID; approval Ya/Tidak/Selalu timeout 120 dtk → deny, klaim-tunggal, audit `telegram_approval` terpisah; Always per-(chat, sesi); chunking 4096 sadar-fence cadangan 14; probe timeout 0 + drain, retry probe bila gagal; 429 retry_after (cap 2× hanya jalur kirim); Ctrl-C abort poll.
  - Bug layering config DIPERBAIKI: section `telegram` sempat hilang saat merge (`merge_into` tidak membawanya) + regression test `test_merge_into_keeps_telegram`.
  - Verifikasi 2 checker independen (diam-diam, tree dibekukan): angka direproduksi persis; **E2E mock 11 kasus, 38/38 asersi PASS** (main, group, poll429, poll401, long, deny, timeout, always, probefail, groupcmd, alwaysnew). Temuan checker DIPERBAIKI batch 2e: Always per-(chat, sesi), cadangan chunk 10→14 (regresi ~440 KB), probe retry + drain, `bot_command` = mention grup, Ctrl-C abort, kasus `always` tidak lagi vacuous (mock bersiklus), label asersi timeout, session ID 12 hex, kartu tanpa fence, persist pesan saat turn gagal, callback dijawab sekali.
  - TERBUKA / hanya live: konektivitas `api.telegram.org` dari PC user, rendering keyboard di aplikasi, 429/grup asli, `ask` picker live, `/resume <id>` tak ada, `/stop` saat turn berjalan, turn paralel lintas chat, `service install` live. Bot `@purrcodeai_bot` dibuat user 2026-11-12; token TIDAK disimpan di mana pun; uji live butuh chat ID user (kirim satu pesan ke bot → baca `getUpdates` → `allow_from`).
  - Hasil ukur: check 0 warning, test 135/135, clippy `-D warnings` bersih, fmt bersih, release GNU 4.826.088 byte (4,60 MiB); suite musl 135/135, rilis musl 4.949.152 byte (4,72 MiB) static-pie stripped, `ldd` = statically linked.

## 11. Milestone v0.3 — Autonomous Intelligence & OS Sandboxing (F19–F22)

- [x] **11.1 F19 cron — permukaan manajemen (batch 3a) 2026-11-12 (workplan item 26, ADR-014) — `crates/purrcode-cli/src/cron/`, `crates/purrcode-config/`**
  - Parser jadwal deterministik ID+EN ala Hermes (cron mentah, ISO one-shot, interval, hari kerja/weekend, marker waktu, relatif, "besok jam 8"); store `~/.purrcode/cron/jobs.jsonc` atomik + delivery queue `deliveries.jsonl` claim-first; CLI `cron add/list/remove/enable/disable/deliveries`; allowlist fail-closed (kosong = tidak ada tool; default = read-only Meowstro); job menolak preset God / approval yolo / tool tak dikenal; guard `sandbox: "os"` (ditolak sampai F21 menegakkannya — menutup lubang downgrade confinement diam-diam); konfirmasi interaktif butuh TTY asli.
  - Verifikasi 2 checker independen (diam-diam, tree dibekukan): angka direproduksi persis (152/152 saat itu; PTY dua arah; parser 8/8 + 10 reject; atomic save 0 `.tmp`). Temuan checker DIPERBAIKI sebelum commit: waktu invalid diam-diam jadi 09:00 → error; detik 90 dtk terpotong jadi 1 menit → ditolak (hanya kelipatan 60 eksak); interval tak ter-ekspresikan disimpan dengan periode salah → `interval_expr` mengembalikan `Result` (menit bagi 60, jam bagi 24, multi-hari selain harian ditolak) + test mengasersi `expr`; EPIPE panic exit 134 → SIGPIPE default (exit 141); `to_expr` step 2-elemen kini `*/n`.
  - TERBUKA (batch 3b): runner job + tick gateway + drain delivery + `cron run`; perilaku one-shot lewat waktu (jangan dieksekusi terlambat); `cron.tick_secs` hasil merge belum punya permukaan CLI.
  - Hasil ukur: check 0 warning, test 153/153, clippy `-D warnings` bersih, fmt bersih, release GNU 4.993.696 byte (4,76 MiB); suite musl 153/153, rilis musl 5.121.184 byte (4,88 MiB) static-pie stripped, `ldd` = statically linked.
- [x] **11.2 F19 cron — runner + tick gateway + delivery (batch 3b) 2026-11-12 (workplan item 27, ADR-014) — `crates/purrcode-cli/src/cron/runner.rs`, `gateway.rs`, `tool.rs`, `approval`, `ask.rs`, `render.rs`**
  - Runner job (`run_job`) di sesi khusus `purr_cron_<id>_<run>`: preset snapshot di-cek ulang saat jalan (God/yolo ditolak lagi), `yolo=false` di kedua tempat, requester deny-always (`CronApprovalRequester` + audit `cron_approval`; Ask-dalam-allowlist dianggap disetujui), `ToolRegistry::retain` dengan allowlist job, `is_cron: true` dipaksa dari `ToolContext` (override model diabaikan), `AskTool` requester-None = error (tanpa fallback stdin), lease "running" + guard enabled di kepala `run_job` (manual `cron run` juga terikat rails yang sama), `manage_skill` dilarang di allowlist (`CRON_FORBIDDEN_TOOLS`, creation + run-time), `send_standalone_reported` + audit `cron_delivery_failed` (at-most-once tetap).
  - Tick gateway 60 dtk (default, min 5) dengan error boundary sendiri + `MissedTickBehavior::Skip`; klasifikasi due/missed (grace recurring `max(2*tick,120s)`, one-shot 300 dtk — tidak pernah fire terlambat); drain delivery claim-first; `cron run` foreground.
  - Verifikasi 2 checker independen (diam-diam, tree dibekukan): angka direproduksi persis (166/166 GNU + musl, clippy/fmt bersih, GNU 5.033.184 byte, musl 5.166.256 byte static-pie). E2E mock **9 kasus, 58/58 asersi PASS** (A basic, B deny, C shadow, D ask, E gateway tick, F concurrent, G disabled, H manage_skill, J black-hole). Temuan checker DIPERBAIKI sebelum commit: re-check F1 (lease/enabled di `run_job`, blocking), re-check F2 (`manage_skill` dilarang), build F1 (lookup job sebelum model), build F2 (audit kegagalan kirim), build F3 (marker tanpa timestamp = basi, self-healing).
  - TERBUKA (dicatat jujur): dua `cron run` dalam milidetik yang sama masih bisa balapan (lease menutup jeda ≥ detik, bukan klaim atomik); `cron run` gagal hanya mencetak summary, bukan detail penuh.
- [x] **11.3 F22 heartbeat/proactive (batch 4a) 2026-11-12 (workplan item 28, ADR-014) — `crates/purrcode-cli/src/heartbeat/`, `gateway.rs`, `main.rs`, `telegram/sessions.rs`, `cron/queue.rs`** — ticker gateway (default 30 menit, min 5), `~/.purrcode/HEARTBEAT.md` (proyek lalu global; template bila hilang), two-phase run/skip (Phase-1 gagal → Skip, deviasi disengaja dari ZeroClaw), sentinel diam, delivery lewat antrean yang sama + routing last-active-PRIVAT (grup tidak pernah jadi target), `PROACTIVE_PREFERENCES.md`, TUI seed pending deliveries, `heartbeat status` (read-only) / `run`. E2E mock 15 kasus 94/94 (termasuk K grup, L/M preset, N disabled, O antrean rusak). Live: tick pertama jalan di gateway 11:49 WIB (template dibuat, tanpa LLM call).
- [x] **11.4 F20 kurator skill self-improving (batch 4b + follow-up) 2026-11-12 (workplan item 29, ADR-014) — `crates/purrcode-cli/src/curator/`, `gateway.rs`, `main.rs`, `telegram/approval.rs`, `approval/src/lib.rs`** — trigger post-turn ≥ 5 tool call (hitung dari audit, marker reset), review fork SATU call tool-less (log di-scrub secret), draft di `~/.purrcode/skills/.drafts/<nama>.<sesi>.md` (session-keyed, tidak dibaca discovery), secret scan + validasi (tanpa `name:` ditolak — bukan jadi skill `"draft"`), approval TUI/Telegram (Selalu terpisah dari approval tool, `sk:`/`skill_draft`), Tinker-only `manage_skill create` (scope `global` eksplisit) saat approve, recursion guard + `consume_save_turn`. E2E mock 7 kasus 49/49 (termasuk G always 11/11). Follow-up memperbaiki fail-open parser verdict Tuxeshadow (kosong/sampah → Object, ditemukan dari perbandingan `dev-claude`) + F1–F4 build checker. Re-checker adversarial gagal dua kali oleh runtime (bukan oleh check) — batch ditutup atas build checker CLOSED + E2E + temuan perbandingan dev-claude.
- [x] **11.5 F21 sandbox OS (AKHIRAN) 2026-11-12 (workplan item 33, ADR-014) — crate `landlock 0.4`, `crates/purrcode-tools/src/execution/sandbox.rs`, `bash.rs`, `crates/purrcode-config/src/preset_mode.rs`** — Landlock `pre_exec → restrict_self()` di spawn `bash` saja (ruleset dibangun di parent; ABI di-pin V8 = kernel 7.0); field `ToolContext.sandbox` baru + preset `sandbox_mode` (read-only/workspace-write/danger-full-access) sebagai strictness ruleset di dalam `sandbox: "os"`; fail-closed bila Landlock tak tersedia / non-Linux; guard preset loader DIBALIK dari MENOLAK `sandbox: "os"` menjadi MENEGAKKANNYA; pre-validasi path rule di parent (crate diam-diam melewati path yang tak bisa dibuka); cron snapshot mem-pin `sandbox_mode` + fail-closed. Keterbatasan jujur: child masih bisa MEMBACA di mana saja (deny-read path secret + seccomp network = follow-up fase 2); Seatbelt macOS desain saja.
  - Verifikasi 2 checker independen (diam-diam, tree dibekukan) + fix pass + re-verifikasi ronde kedua: build checker CLOSED (suite 226/226 saat itu — hitungan implementor 224 adalah kesalahan aritmetika, bukan defect; GNU 5.262.184 byte, musl 5.391.536 byte, E2E 6/6). Re-checker adversarial: Landlock bertahan di semua serangan eksekusi — **tapi 1 P1 live, bukan di Landlock**: preset `sandbox_mode` tak dikenal jatuh senyap ke tabby, bash jalan TANPA sandbox. Fix pass: `load_by_name_or_tabby` (fallback tabby HANYA bila file preset tidak ada; preset yang ada tapi gagal parse = hard error) di 5 titik entry point + 2 unit test + E2E kasus ke-7. Re-verifikasi: build checker #2 CLOSED (228/228, GNU 5.261.448 byte, musl 5.391.536 byte, E2E 7/7), re-checker #2 CLOSED (P1-1 tertutup live, probe Landlock 12/12, 1 P3 komentar basi diperbaiki).
  - Hasil ukur: check 0 warning, test 228/228, clippy `-D warnings` bersih, fmt bersih, release GNU 5.261.448 byte; suite musl 228/228, rilis musl 5.391.536 byte static-pie stripped, `ldd` = statically linked. E2E mock 7 kasus 7/7 PASS.

## 12. Refactor merge basis dev-muse (R1–R3) — keputusan user 2026-11-12 17:45 WIB (basis dev-muse, bukan dev-claude)

- [x] **12.1 R1 — ADR-011 file splits (workplan item 30, AUDIT_STATUS item 25)** — main.rs/task.rs dipecah logika sungguhan + schedule/runner/loop_engine pindah test; semua file .rs <800 baris; API publik tidak berubah (re-export); 2 checker independen CLOSED (build + re-check adversarial) + paritas live vs `d41dca0` identik (TUI 10/10, one-shot 10/10, cron 7/7, gateway tick 7/7, multiset audit sama); E2E 49/49. Utang terlihat: schedule/mod.rs 790 + runner/mod.rs 766 margin tipis (logika tidak dipecah — fitur berikutnya yang menyentuh mereka harus pecah logika); skill/mod.rs 716 + heartbeat/runner.rs 709 juga dekat batas.
- [x] **12.2 R2 — ekstraksi service** — pindah cron/curator/heartbeat dari `crates/purrcode-cli/src/` ke `crates/purrcode-service/src/` dengan wiring AKTIF ke ApprovalGate (enforcement ikut pindah, bukan hanya file); service TIDAK BOLEH depend ke CLI (one-way chain, AGENTS.md §6) — cron runner saat ini memakai helper CLI (`build_runner_deps` mencerminkan setup run_one_shot di main/one_shot), jadi dependensi bersama harus turun ke service crate (atau crate yang lebih rendah), CLI hanya menyimpan wiring/dispatch perintah.
 **SELESAI 2026-11-12** — terverifikasi 2 checker independen (build CLOSED + re-check adversarial CLOSED 98/99), AUDIT_STATUS.md item 26; commit lokal menyusul, push butuh persetujuan user.
- [x] **12.3 R3 — event bus hidup + port WebUI** — hidupkan EventBus di loop_engine dev-muse (6 publish sites referensi dev-claude: TurnStarted/ReasoningChunk/TextChunk/ToolCallStarted/ToolCallApprovalRequested/ToolCallFinished) + port WebUI Catppuccin dari worktree dev-claude (`crates/purrcode-channel/src/web/` 1.246 baris) dan wire ke run_turn/AgentEventListener + event bus yang kini hidup; tambah subcommand `purrcode web` (localhost-by-default, auth Bearer/?token=). R3 juga membuka jalan D5 (event stream `--json` penuh). Worktree dev-claude dihapus setelah merge thread selesai. **SELESAI 2026-11-12 (2 checker diam-diam + fix pass):** suite 217/217, clippy/fmt bersih, GNU 5.245.744 byte, musl 5.375.152 byte; build checker CLOSED (E2E 20/20 dari salinannya), re-checker menemukan 3 P1 + 1 P2 live → diperbaiki (kartu shadow, split-read, XSS, token scope) + diverifikasi ulang (driver 127/131, 4 FAIL = inversi perbaikan; split-probe PASS). AUDIT_STATUS item 27 + §1 + §3.
