# PurrCode AI — Architecture Decision Records (DECISIONS.md)

Status: Living Document  
Lisensi: **GPL-3.0** | Bahasa: **Rust** | Binary: **`purrcode-ai`**

---

## Ringkasan Keputusan (ADR Index)

| ADR | Judul Keputusan | Tanggal | Status |
|---|---|---|---|
| ADR-001 | Bahasa Implementasi: Rust (Single Static musl Binary) | 2026-11-11 | ✅ Diterima |
| ADR-002 | Lisensi Proyek: GNU GPL v3 | 2026-11-11 | ✅ Diterima |
| ADR-003 | Bahasa Balasan Agen: Adaptif (Mengikuti Pengguna & Model) | 2026-11-11 | ✅ Diterima |
| ADR-004 | Nama Binary CLI: `purrcode-ai` & Repo Publik | 2026-11-11 | ✅ Diterima |
| ADR-005 | Model Default: Zero Hardcode, Konfigurasi Eksplisit via Custom Endpoint | 2026-11-11 | ✅ Diterima |
| ADR-006 | Anti-Hilang-Konteks Subagent: Continuation ID + Handoff Files + Registry | 2026-11-11 | ✅ Diterima |
| ADR-007 | Meowstro: Orchestrator Read-Only Ditegakkan Runtime Tanpa Write/Edit/Bash | 2026-11-11 | ✅ Diterima |
| ADR-008 | Preset Mode & Guardrail 3 Sumbu: Carrier (Strict), Tabby (Default), Oyen (God) | 2026-11-11 | ✅ Diterima |
| ADR-009 | Preset Model Terpisah dari Mode: Free, Local, Premium, dan Custom User Files | 2026-11-11 | ✅ Diterima |
| ADR-010 | Memori Default Markdown Minimal Tanpa Database Wajib | 2026-11-11 | ✅ Diterima |
| ADR-011 | Standar File Kode: Hard Limit 800–1000 Baris Sebelum Split Submodul | 2026-11-11 | ✅ Diterima |
| ADR-012 | Tuxeshadow: The Shadow Auditor (Pengawas Independen Meowstro) | 2026-11-11 | ✅ Diterima |

---

### ADR-001: Bahasa Implementasi Rust (musl statis)
- **Konteks:** Perangkat target mencakup PC dev, server VPS, OpenWrt router/STB 2 GB RAM, dan HP postmarketOS (3.5 GB RAM). Dibutuhkan binary tunggal tanpa runtime dependency (tanpa Node.js, Python, atau Bun).
- **Keputusan:** Rust dipilih dengan kompilasi musl statis (`cargo zigbuild`). Stack: Tokio (async), Ratatui (TUI), Rusqlite (session index), Reqwest + Rustls (HTTP tanpa dependensi libssl OS).
- **Konsekuensi:** Kompilasi lebih lambat, namun runtime memory idle ≤ 50 MB, startup < 1 detik, footprint binary ≤ 25 MB.

### ADR-002: Lisensi Proyek GNU GPL v3
- **Konteks:** Menjaga kebebasan software open-source dan mencegah privatisasi harness tanpa kontribusi balik.
- **Keputusan:** Seluruh codebase dilisensikan di bawah GPL-3.0. Seluruh dependensi Rust wajib kompatibel GPL. Plugin eksternal proprietary diakomodasi via proses terpisah melalui JSON-RPC stdio agar tidak melanggar lisensi linked code.

### ADR-003: Bahasa Balasan Agen Adaptif
- **Konteks:** Agen sebelumnya diusulkan mengunci bahasa balasan tertentu.
- **Keputusan:** Bahasa balasan agen bersifat adaptif mengikuti bahasa yang digunakan pengguna dalam percakapan dan model yang digunakan. Kode, komentar, identifier, dan log internal tetap ditulis dalam bahasa Inggris.

### ADR-004: Nama Binary CLI `purrcode-ai` & Repo Publik
- **Konteks:** Menentukan nama perintah command-line dan visibilitas repository.
- **Keputusan:** Perintah utama terminal adalah `purrcode-ai`. Repo di GitHub bersifat publik (`latifangren/purrcode-ai`).

### ADR-005: Model Default: Zero Hardcode, Konfigurasi Eksplisit
- **Konteks:** Beragamnya setup pengguna (menggunakan router proxy lokal seperti Magpie di `http://127.0.0.1:3425/v1`, LAN proxy 9router/omniroute, atau Ollama lokal).
- **Keputusan:** Tidak ada default model yang di-hardcode ke binary. Binary menolak menjalankan prompt bila belum ada konfigurasi model. Pengguna wajib mengonfigurasi provider/custom endpoint di `~/.purrcode/purrcode.jsonc` atau dipandu interaktif via `purrcode-ai onboard`.

### ADR-006: Subagent Anti-Lost-Context
- **Konteks:** Pada oh-my-opencode-slim, pencegahan hilang konteks hanya bergantung pada kepatuhan prompt LLM yang rapuh.
- **Keputusan:** Harness menegakkan ID delegasi stabil (`purr_<id>`), penulisan file handoff (`brief.md` dan `result.md`), serta registry sesi JSON. Panggilan berulang ke ID yang sama akan melanjutkan child yang ada, bukan melakukan respawn.

### ADR-007: Meowstro Orchestrator Read-Only
- **Konteks:** Meowstro memegang konteks utama percakapan. Jika Meowstro menulis kode, diff dan error output akan memenuhi konteks dan memicu compaction yang merusak rencana kerja.
- **Keputusan:** Tool set Meowstro dibatasi oleh harness pada saat pendaftaran tool: hanya memiliki akses `read`, `grep`, `glob`, `find`, `todo`, `ask`, dan `task`. Tool `write`, `edit`, dan `bash` tidak didaftarkan dan ditolak runtime jika dipanggil. Implementasi selalu didelegasikan ke Tinker.

### ADR-008: Preset Mode 3 Sumbu (Carrier, Tabby, Oyen)
- **Konteks:** Menghindari penggabungan antara level persetujuan (approval), batas konten (guardrail), dan isolasi sistem (sandbox).
- **Keputusan:** Preset berupa file JSONC berskema `preset/v1`. Memisahkan sumbu Approval (manual/supervised/auto/yolo), Guardrail (strict/default/god), dan Sandbox (confinement/os). Penolakan kebijakan provider (ToS) tidak pernah di-bypass melalui fallback.

### ADR-009: Preset Model Terpisah dari Mode
- **Konteks:** Pengguna perlu mengatur inferensi model per subagent tanpa mengubah izin dan mode keamanan.
- **Keputusan:** File berskema `model-preset/v1` (`model-presets/*.jsonc`) mengatur model, temperature, dan thinking variant per subagent. Snapshot preset model disimpan per sesi untuk menjamin stabilitas eksekusi.

### ADR-010: Memori Default Markdown Minimal
- **Konteks:** Menghindari dependensi wajib pada database vector atau database berat saat instalasi awal.
- **Keputusan:** Format memori default adalah file Markdown (`MEMORY.md`, `USER.md`, `SOUL.md`). Backend eksternal atau FTS5 berjalan di belakang trait berversi `MemoryBackend` dengan aturan *dual-write* ke Markdown.

### ADR-011: Standar File Kode 800–1000 Baris
- **Konteks:** Memastikan arsitektur modular, rapi, dan mudah dibaca/di-maintain.
- **Keputusan:** Setiap file source code memiliki batas keras (hard limit) 800–1000 baris. Modul yang mendekati batas ini wajib dipecah menjadi subdirektori dengan tanggung jawab terfokus.

### ADR-012: Tuxeshadow (The Shadow Auditor)
- **Konteks:** Meowstro adalah *single point of failure*. Jika rencananya salah, Tinker akan mengeksekusi kesalahan tersebut dan memperbaikinya post-facto jauh lebih mahal. Butuh pengawas independen (*Actor-Critic pattern* / *Independent Oracle*).
- **Keputusan:**
  1. Diadopsi sebagai agen ke-7 di The Clowder (`.purrcode/agents/tuxeshadow.md`).
  2. Toolset read-only: `read`, `grep`, `glob`, `find`, `ask` — tanpa `write`, `edit`, `bash`, dan **tanpa `task`** (rantai komando delegasi tetap tunggal di Meowstro).
  3. Trigger sempit (event-driven): Sebelum rencana disetujui, sebelum aksi destruktif (tulis di luar workspace, hapus file, eksekusi command bahaya), saat recovery plan setelah error, dan wajib pada cron job otomatis.
  4. Bekerja atas artefak mentah (plan JSON, brief, diff asli), bukan ringkasan kata-kata Meowstro (menghindari framing bias).
  5. Sifat putusan: Non-binding secara default. Meowstro dapat menerima keberatan atau meng-override dengan alasan tercatat di audit log (`⚠ [shadow overridden]`).
  6. Perilaku per mode: Carrier = memblokir (butuh approval user); Tabby = advisory; Oyen = silent audit log.
  7. Kelas Model: Wajib menggunakan model penalaran kuat (*high reasoning effort*, setara atau di atas Meowstro, sekelas Sphinx). Auditor tidak dapat menemukan cacat logika jika modelnya lebih lemah dari perancang rencana. Biaya token terkendali lewat trigger sempit (hanya pada checkpoint krusial) dan output putusan ringkas.

### ADR-013: Telegram Gateway v0.2 (F15–F18)
- **Konteks:** Telegram adalah channel pertama PurrCode (PRD §4.2 v0.2, ala Hermes Agent). Harus jalan sebagai daemon `purrcode gateway` dengan satu memori, approval fail-closed, dan sesi per chat — tanpa SDK berat dan tanpa mengorbankan disiplin keamanan v0.1 (fail-closed, audit, confinement).
- **Keputusan:**
  1. Klien Bot API hand-rolled di atas reqwest (PRD F15, ala ZeroClaw); tanpa teloxide. `api_base` bisa dikonfigurasi, default `https://api.telegram.org` — diperlukan agar pengujian offline (mock server lokal) mungkin. **Deviasi disengaja dari pola SSRF webfetch:** IP-pinning (resolve → validasi → pin) TIDAK berlaku untuk `api_base` — (a) tidak ada URL kendali-model yang pernah mencapai klien ini (path tetap, hanya base yang dikonfigurasi user yang bervariasi), (b) `base_url` provider diperlakukan dengan cara yang sama (tanpa pin), (c) pinning merusak Bot API server self-hosted di IP LAN dan mock lokal. Klien hanya memvalidasi skema http/https + token tak-kosong.
  2. `allow_from` fail-closed (F18): daftar kosong = tidak ada yang boleh. Dicek sebelum pekerjaan lain; chat tak dikenal mendapat SATU balasan "bot ini privat" lalu diabaikan; isi pesannya tidak pernah masuk log/audit.
  3. Kunci sesi = chat ID (DM = user ID; grup = chat ID grup). Bukan `from.id`, bukan thread/topik (OpenHuman `history_key_ignores_thread`).
  4. Approval (F17): tombol inline Ya / Tidak / Selalu (sesi ini); timeout 120 dtk → deny. Resolusi klaim-tunggal: siapa yang berhasil menghapus entri pending, dialah yang menentukan hasil (menutup race tap-vs-timeout). Selalu = allowlist tool per-(chat, sesi) di dalam requester — dikunci oleh `(chat_id, session_id)`, jadi sesi baru lewat `/new` mulai bersih ("sesi ini" berarti harfiah; Always dari sesi lama tidak ikut auto-approve di sesi baru). Audit: selain entri generik `tool_approval` dari loop, requester menulis entri `telegram_approval` dengan keputusan `approved_by_user` / `denied_by_user` / `approved_always` / `denied_timeout` / `denied_unreachable` — sumber (operator vs timeout) tidak boleh disamakan. Catatan: entri generik loop memetakan bool ke `denied_by_user` juga saat timeout (bool tidak membawa sumber) — sumber presisi hanya ada di `telegram_approval`.
  5. `ask` memakai mesin callback yang sama (`TelegramAskRequester`, timeout → `None`).
  6. Rendering lewat `AgentEventListener` (jalur yang dipakai TUI), bukan `EventBus` (yang belum dipakai siapa pun). **Koreksi atas klaim awal:** listener TIDAK diperluas dengan `on_subagent_event` — tidak ada di kode (trait hanya punya 3 callback: text/tool started/tool finished); baris status subagent (DESIGN §3, "🐾 Whisker selesai") DITUNDA. Trait `Channel` dipertahankan bentuknya (render + approval); inbound `listen()` tinggal di `TelegramChannel` konkret — trait belum punya konsep inbound, dan memaksakannya sekarang berarti mendesain inbound WebUI v0.4 secara prematur.
  7. Pesan: plain text dulu (tanpa `parse_mode` pada keluaran model); chunking 4096 karakter, sadar code fence, marker `(i/n)`; edit draft di-throttle ~1/dtk, hanya bila konten berubah; error "message is not modified" diabaikan. Cadangan chunk 14 karakter (bukan 10): marker 3-digit `(999/999)` = 10 + penutup fence sintetis = 4 — cadangan 10 terbukti kurang (pesan berpagar ~440 KB menghasilkan chunk sampai 4100 karakter, ditolak API asli dengan 400).
  8. Token lewat `${TELEGRAM_BOT_TOKEN}` dari `~/.purrcode/credentials` (0600) via `expand_env_vars` yang sudah ada; tidak pernah di `purrcode.jsonc`. Token di-redaksi di `Debug`/log (URL ber-token di-scrub).
  9. Poll: probe `timeout: 0` dulu, lalu loop `timeout: 30`, `limit: 100`, `allowed_updates: ["message", "callback_query"]`. Offset maju hanya setelah satu batch berhasil diproses semua. 429 → tunggu `retry_after` (maks 60 dtk). **Klarifikasi:** cap "maks 2 retry" hanya berlaku di jalur KIRIM (`send_message`/`edit`/`answer_callback_query`); loop poll mengulang 429 tanpa batas (benar untuk daemon). 401 → berhenti dengan pesan jelas. Error I/O → log + tidur 5 dtk + ulangi. **Probe gagal (non-401) diulang sampai berhasil / 401 / shutdown** — tidak pernah masuk poll loop dengan offset `None` (poll pertama akan MENJALANKAN backlog basi, bukan men-drain-nya).
  10. Turn berjalan berurutan per chat, paralel antar chat (satu task per chat). Ctrl-C meng-abort task poll (shutdown tidak menunggu `getUpdates` yang tertahan sampai `poll_timeout + 15 dtk`, dan tidak menunggu turn yang sedang berjalan — daemon keluar dengan turn di tengah jalan).

### ADR-014: v0.3 — Cron (F19), Kurator Skill (F20), Sandbox OS (F21), Heartbeat (F22)
- **Konteks:** PRD §4.3 v0.3. Riset referensi selesai 2026-11-12 (`~/workspace/purrcode-ai-v03-research.md`, 424 baris): Hermes paling dekat semantik F19 (parser NL deterministik, allowlist fail-closed, delivery queue durable); ZeroClaw paling lengkap bentuk Rust-nya (poll loop, default-excluded cron tools, heartbeat two-phase, Landlock-in-`pre_exec`); PicoClaw pola heartbeat sederhana + evolusi skill; tidak ada referensi yang mengimplementasikan F20 verbatim.
- **Keputusan:**
  1. Urutan implementasi: **F19 → F22 → F20 → F21.** F19 dulu (nilai terbesar + membangun tick daemon + delivery queue yang dipakai F22); F22 kedua (hampir gratis setelah F19); F20 ketiga (independen, kecil); F21 terakhir (tersulit + sandbox adalah "akhiran" per arahan user 2026-11-12).
  2. **F19 store:** definisi job di `~/.purrcode/cron/jobs.jsonc` (id, nama, schedule ter-parse + `schedule_display`, prompt, allowlist tool, snapshot preset, delivery, enabled, last/next run); delivery queue append-only `~/.purrcode/cron/deliveries.jsonl`. Klaim at-most-once ala Hermes: append record `claimed` → kirim → append `delivered`; `claimed` tanpa `delivered` dianggap terkirim (lebih baik hilang daripada ganda).
  3. **F19 NL parser:** deterministik ala Hermes (tabel weekday/waktu + durasi + cron expr + ISO), **Indonesia + Inggris** (contoh PRD berbahasa Indonesia); echo `schedule_display` + `next_run_at` saat job dibuat supaya user melihat apa yang dipahami.
  4. **F19 safety (mengikat):** job jalan sebagai Meowstro di sesi khusus `purr_cron_<id>_<run>`; preset snapshot dari pembuatan — tolak guardrail God dan preset (termasuk custom) yang mendeklarasikan `approval: "yolo"`; `yolo=false` di kedua tempat (`with_complete_options` dan `from_preset`); requester deny-always (tidak ada manusia untuk mengetuk kartu); allowlist job menggantikan daftar tool preset via `ToolRegistry::retain`; **tool di allowlist yang gate-nya Ask dianggap disetujui** (user sudah menyetujui spesifik itu saat membuat job — itulah arti allowlist); di luar allowlist → job gagal + notifikasi lewat delivery queue; job tidak boleh mengelola cron (default-excluded: `cron_add`, `cron_update`, `cron_remove`, `cron_run`, `schedule` — ala ZeroClaw) dan tidak boleh mendeklarasikan `manage_skill` di allowlist (skill hanya dibuat lewat F20 dengan persetujuan user; ditempel 2026-11-12 setelah temuan re-checker 3b); Tuxeshadow wajib (`is_cron=true`, objection → gagal fail-closed); daemon tidak pernah memanggil `confirm_god_mode_if_needed` (blokir stdin).
  5. **F19 daemon:** tick di dalam gateway dulu (interval 60 dtk, **error boundary sendiri** — kegagalan scheduler tidak boleh mematikan poll Telegram dan sebaliknya); scheduler sebagai modul terpisah (`crates/purrcode-service/src/cron/` — dipindah dari `purrcode-cli` ke `purrcode-service` di R2 refactor merge 2026-11-12) agar bisa dipecah jadi daemon mandiri nanti. CLI: `purrcode cron add|list|remove|enable|disable|run|deliveries`. `service install` tetap generate-only di v0.3 (install nyata = pekerjaan terpisah, bukan bagian F19).
  6. **F22 heartbeat:** ticker di gateway (default 30 menit, min 5); baca `~/.purrcode/HEARTBEAT.md` (atau `.purrcode/HEARTBEAT.md` proyek); two-phase ala ZeroClaw (Phase-1 putuskan `run: …` / `skip`, konservatif; Phase-2 jalankan yang dipilih); sentinel sunyi (`HEARTBEAT_OK` / `NO_REPLY`); delivery ke chat aktif terakhir (Telegram) atau tampil saat TUI dibuka; allow/deny via `~/.purrcode/PROACTIVE_PREFERENCES.md` (Tell me about / Never tell me about / jendela waktu).
  7. **F20 kurator:** trigger post-turn ≥ 5 tool call (counter, bukan penilaian); draft dari log tool-call turn itu (session JSONL + audit JSONL); fork review read-only (tanpa `manage_skill`); draft di-stage di `~/.purrcode/skills/.drafts/<name>.md` (discovery mengabaikan — tidak bocor ke prompt index); secret scan + validasi struktural sebelum ditampilkan; approval via kartu TUI / tombol Telegram (120 dtk → deny); on approve, Tinker jalankan `manage_skill action=create`; on deny, buang + log audit; recursion guard (review fork tidak memicu review lagi); skip turn heartbeat/cron.
  8. **F21 sandbox:** Landlock-in-`pre_exec` pada child `bash` saja (bukan proses agen — Landlock diwariskan dan irreversibel); satu crate baru `landlock 0.4`; `sandbox: os` diaktifkan lewat field baru di `ToolContext` + branching di `bash.rs` spawn; mode `read-only` / `workspace-write` / `danger-full-access` adalah strictness ruleset di dalam `sandbox: os` (field preset `sandbox_mode`, bukan nilai sumbu baru); fail-closed bila Landlock tak tersedia; in-process `write`/`edit` dan `webfetch`/`websearch` tetap approval + confinement; restriksi network (seccomp) = fase 2 setelah v0.3. **Lubang hari ini:** nilai `sandbox: "os"` saat ini parse tapi no-op dan diam-diam melemahkan confinement — sampai F21 mendarat, preset loader **menolak** `sandbox: "os"` dengan pesan jelas (bukan diam-diam mengizinkan).
  9. **Config merge:** setiap section baru (cron/heartbeat) wajib punya merge arm di `merge_into` + regression test di commit yang sama (pelajaran dari Telegram — section hilang saat merge).
  10. **Delivery bersama:** hasil cron + heartbeat masuk delivery queue yang sama; gateway mengirim ke home chat (Telegram: `allow_from` pertama / chat aktif terakhir); TUI menampilkan pending deliveries saat dibuka.
- **Amandemen F22 (ditempel 2026-11-12, batch 4a — penyempurnaan dari implementasi, bukan perubahan arah):**
  - Phase-1 gagal (error provider / jawaban tak ter-parse / indeks di luar jangkauan) → **skip**, bukan jalankan semua (deviasi disengaja dari ZeroClaw yang menjalankan semua tugas saat Phase-1 gagal: eksekusi tanpa pengawasan tidak boleh default ke "jalankan semuanya" saat decider-nya gagal).
  - Guard preset God/yolo dicek sekali di awal tick (sebelum Phase-1, tanpa LLM call), bukan per-item.
  - Turn Phase-2 adalah turn daemon tanpa pengawasan seperti cron: `is_cron: true` di `ToolContext` (Tuxeshadow wajib, override reason dari model diabaikan), allowlist read-only default Meowstro, `yolo=false`, requester deny-always (`CronApprovalRequester`/`CronAskRequester` dipakai ulang).
  - "Last active chat" dilacak di `chats.json` (`last_active_at`, Unix seconds, di-touch pada setiap pesan/perintah yang diterima); fallback deterministik = `allow_from[0]` (urutan config, bukan `HashSet` channel).
  - "Last active chat" untuk DELIVERY hanya chat **privat**: `chats.json` menyimpan `kind` (`private`/`group`/`supergroup`/`channel`, dicatat saat touch) dan `last_active_private_chat()` mengembalikan hanya chat privat — grup/supergroup/channel dan entri tanpa kind tidak eligible (fail-closed; fallback `allow_from[0]` menutupi kasus user tunggal). Alasan: ringkasan heartbeat bisa mengutip pengecekan privat user (contoh PRD: ringkasan email) — mengirimkannya ke grup adalah kebocoran ke anggota grup (temu re-checker F1, batch 4a fix pass, 2026-11-12).
  - Preset yang gagal load (nama tak ada, file tak ter-parse, `sandbox: "os"`) membuat tick heartbeat GAGAL dengan pesan — tidak ada silent fallback ke tabby di jalur heartbeat (pola fallback di `gateway.rs`/`main.rs` adalah pra-4a dan sengaja tidak diubah batch ini).
  - `heartbeat run` menghormati flag `[heartbeat] enabled = false` (seperti tick gateway); `heartbeat status` tetap jalan (read-only).
  - Antrean delivery tahan baris rusak: `read_events` melewati baris yang gagal parse dengan `tracing::warn` (delivery pada baris itu hilang, sisanya tetap termuat) — satu baris rusak tidak lagi melumpuhkan permanen antrean.
  - Di luar jendela waktu: seluruh tick dilewati SEBELUM ada LLM call (bukan hanya deliverynya yang ditunda). File `HEARTBEAT.md` yang hilang → template global dibuat sekali, tick berakhir sunyi tanpa LLM call.
