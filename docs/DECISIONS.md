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
