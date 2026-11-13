# Rencana v0.4 — WebUI khusus (F23–F25)

> Status: **PLAN ONLY** — ditulis 2026-11-13, belum ada implementasi.
> Dasar: PRD §4.4, DESIGN §4, AUDIT_STATUS §1 (baris v0.4), hasil R3
> (AUDIT_STATUS item 27, ROADMAP 12.3). Cabang kerja: `dev-muse` @ `e9730cd`
> (v0.3 100%, sudah push). Aturan kerja yang berlaku: tanpa laporan per-batch
> (checker diam-diam, temuan jadi fix task — arahan user 2026-11-12); push
> hanya atas persetujuan per-aksi; ADR-011 (<800 baris/file); free-first.

## 1. Tujuan & definisi selesai (dari PRD §4.4)

- **F23 — `purrcode web`:** WebUI lokal khusus PurrCode (bukan desktop app)
  — daftar sesi, transcript + tool card yang sama dengan TUI, manajemen plugin
  (mount/unmount dari UI), manajemen sesi/skill/memory, dan penjadwalan tugas.
  Satu engine yang sama dengan TUI/Telegram (core + event bus + REST/SSE);
  WebUI adalah channel tipis seperti yang lain, **bukan core kedua**.
- **F24 — bukan pengganti TUI:** TUI tetap surface utama untuk coding; WebUI
  untuk pemantauan jarak jauh, manajemen plugin/sesi, dan pemakaian dari HP.
- **F25 — keamanan:** hanya listen di localhost secara default; akses jarak
  jauh hanya via token + TLS (atau reverse proxy user sendiri); tanpa
  akun/cloud.

**Definisi selesai v0.4** (diukur, bukan dirasa): semua halaman DESIGN §4
berfungsi end-to-end dari UI (bukan hanya read-only); multi-sesi web dengan
replay; plugin mount/unmount + hot-reload tanpa restart gateway; keamanan F25
terpenuhi (localhost default, token, session timeout); suite hijau + clippy/
fmt bersih + E2E mock + 2 checker CLOSED per batch; binary tetap ≤25 MB.

## 2. Titik awal — apa yang sudah ada dari R3

| Bagian | Sudah ada (R3, terverifikasi) |
|---|---|
| Tab UI (Catppuccin, static + SSE) | Terminal & Chat, Cron Scheduler, Skills & Curator, Memory & Identity, Plugins & Sandbox (`html.rs` 713 baris) |
| Endpoint | GET `/`, `/api/status`, `/api/events` (SSE), `/api/cron`, `/api/skills` (+candidates), `/api/plugins`, `/api/memory?file=`; POST `/api/prompt`, `/api/approval`, `/api/skills/approve`, `/api/skills/reject` |
| Server | std + **tokio** (`TcpListener`, `tokio::spawn` per koneksi — sudah async; bukan single-threaded) + framed read 1 MiB → 413; `server/mod.rs` 651 baris |
| EventBus | 7 varian hidup di loop; `TextChunk`/`ReasoningChunk`/`ToolCall*`/`TurnStarted`/`TurnFinished`; satu `turn_id` |
| Auth | token exact-match (`Authorization: Bearer <token>` header atau `?token=<token>`); header-only, tanpa trailing junk |
| XSS | `addEventListener` + `dataset`, `escapeHtml` (termasuk `'`), semua field di-escape |
| Sesi | **satu** sesi tetap `purr_web`; `--direct` → AgentLoop polos |
| Keamanan | listen `127.0.0.1` default; token opsional |

Batasan R3 yang sudah dicatat (bukan temuan baru): SSE **live-only** — klien
yang connect di tengah turn melewatkan prefix (tidak ada replay); tidak ada
`on_subagent_event` (status subagent tidak sampai ke UI); `TurnFinished`
selalu `tokens_used: None`.

## 3. Gap — R3 → F23–F25

| # | Gap | Bukti di tree |
|---|---|---|
| G1 | **Sesi: satu, tanpa daftar/switch/resume/replay** | `purr_web` hardcoded; `SessionStore::list_sessions()` sudah ada (`crates/purrcode-core/src/session.rs:182`) tapi tidak dipakai web; tidak ada `GET /api/sessions` / transcript |
| G2 | **Inspector tool card + edit preview** | kartu approval ada (`ToolCallApprovalRequested`), tapi tidak ada panel inspector (detail tool call, diff preview, Terapkan/Tolak) seperti DESIGN §4 |
| G3 | **Status subagent live** | varian `SubagentSpawned`/`SubagentStatusChanged` sudah ada di `AgentEvent`, tapi **tidak ada titik emisi** di loop/task tool (deferred `on_subagent_event`) |
| G4 | **Plugin: list saja** | tab Plugins hanya baca `/api/plugins`; tidak ada `mount`/`unmount` di `purrcode-plugin`, tidak ada daftar hook/config, tidak ada hot-reload |
| G5 | **Memory: read-only** | hanya GET `/api/memory?file=`; tidak ada POST save `MEMORY.md`, tidak ada tampilan fakta backend aktif |
| G6 | **Skill: hanya kandidat kurator** | approve/reject draft saja; tidak ada daftar SKILL.md penuh, tambah/edit skill dari UI |
| G7 | **Scheduler: list saja** | GET `/api/cron` baca job; tidak ada tambah job (NL) / enable/disable/remove dari UI |
| G8 | **Server: routing manual** | routing `match` tangan di `server/mod.rs`; AUDIT_STATUS v0.4 menuntut migrasi Axum/Actix — belum diputuskan frameworknya |
| G9 | **Keamanan F25 belum penuh** | token ada, tapi: tanpa session timeout, tanpa TLS built-in (remote = token saja atau reverse proxy user), tanpa rate limit |
| G10 | **Mobile** | F24 menuntut pemakaian dari HP; tidak ada mobile drawer / layout responsif |
| G11 | **Dashboard canvas** | AUDIT_STATUS v0.4 menyebut "web dashboard canvas" — belum didefinisikan; diusulkan sebagai halaman overview (kesehatan gateway, sesi aktif, cron berikutnya, delivery terakhir) |

## 4. Keputusan desain (kunci sebelum mulai)

| # | Keputusan | Rekomendasi + alasan |
|---|---|---|
| D1 | **Framework server: Axum vs Actix vs tetap std+tokio** | **Axum — DIPUTUSKAN user 2026-11-13 16:03 WIB ("Push 4adbf54 + lanjut B2 pakai Axum").** Server sudah tokio; Axum = routing + extractor di atas tokio tanpa runtime kedua (Actix membawa runtime/ekosistem sendiri). Tetap std = routing manual makin mahal tiap endpoint baru. Risiko: dependensi baru → ukuran binary + waktu build; ukur di B2, batalkan bila binary mendekati 25 MB. **Hasil B2:** Axum menambah ~200 KB (GNU 5.299.648 → 5.501.128 byte) — jauh dari pagu, risiko tidak terjadi. |
| D2 | **TLS: built-in vs reverse-proxy-only** | **Reverse-proxy-only dulu.** F25 mengizinkan "token + TLS (atau reverse proxy user sendiri)". TLS built-in (rustls + manajemen sertifikat) adalah scope besar tersendiri; token + localhost-default + dokumentasi reverse proxy memenuhi F25. Bila user minta remote tanpa proxy, TLS jadi batch susulan. |
| D3 | **Replay transcript** | **Baca dari store yang sudah ada.** Sesi web parent = JSONL (`sessions/purr_web.jsonl`); sesi lain = direktori `sessions/purr_<id>/` + `history.json`. Endpoint `GET /api/sessions` (dari `list_sessions()`) + `GET /api/sessions/:id/transcript`. SSE tetap live-only; replay = HTTP biasa saat buka sesi. |
| D4 | **Multi-sesi web** | **N sesi independen, satu AgentLoop per sesi + turn lock per sesi.** Sesi dibuat dari UI (prompt pertama atau tombol sesi baru); setiap sesi punya `session_id` sendiri (`purr_web_<hex>`), history sendiri, approval sendiri. Batas default: tanpa batas keras dulu, ukur RAM (target idle ≤50 MB tetap berlaku). |
| D5 | **Hot-reload plugin** | **Proses-plugin dulu.** Mount/unmount runtime + reload manifest tanpa restart gateway. Plugin WASM bukan scope (tidak ada host WASM di v0.1–v0.3). |
| D6 | **Frontend** | **Tetap static + SSE, tanpa framework** (sesuai DESIGN §4: "usulan, bukan keputusan final" — plan ini menguncinya: tanpa framework). Satu file `html.rs` + `html.js` yang di-split bila menyentuh ADR-011. |

## 5. Urutan batch (dependency order)

```
B1 fondasi sesi ──> B2 migrasi server ──> B3 inspector + subagent live
  │                                     │
  └─> B4 plugin hot-reload ──> B5 memory/skill ──> B6 scheduler UI
                                                        │
B7 mobile + dashboard canvas ──> B8 keamanan F25 ──> close-out v0.4
```

| Batch | Isi | Prasyarat | Perkiraan risiko |
|---|---|---|---|
| **B1 — Fondasi sesi ✅ SELESAI 2026-11-13** | `GET /api/sessions`, `POST /api/sessions` (baru), `GET /api/sessions/:id/transcript` (dari JSONL/dir), switch sesi di UI (sidebar Sesi), resume sesi lama, replay saat buka sesi | — (bisa jalan di server std) | sedang: format history web (JSONL) vs sesi TUI (direktori) — samakan pembaca, jangan duplikasi format. **Hasil:** registry + endpoint + UI selesai; 2 checker × 3 ronde (ronde 3 CLOSED keduanya), suite 256/256, GNU 5.299.648 byte, musl 5.428.400 byte, E2E 18/18. Fix pass: resume ensure-on-demand, scan parity, cap eviksi 64 + lewati sesi sibuk, non-string 400. Detail: AUDIT_STATUS item 31, ROADMAP 13.1, workplan item 36. |
| **B2 — Migrasi server ✅ SELESAI 2026-11-13** | Pindah routing ke Axum (D1); pecah `server/mod.rs` 651 + `html.rs` 713 sebelum tambah route (ADR-011); pertahankan semua perilaku R3 (framing 1 MiB → 413, token header-only, XSS) + test tripwire yang sama | B1 (supaya route baru ikut pola Axum, bukan std) | sedang-tinggi: regresi perilaku R3 (split-read, token scope, XSS) — E2E R3 harus hijau dulu sebagai baseline. **Hasil:** Axum 0.8 + tower-http limit; `mod.rs` 779 → 5 file; 2 checker × 2 ronde; ronde 1 menemukan P1 Ctrl-C menggantung (diperbaiki: SSE berakhir saat cancel + backstop 5 detik + tes regresi); ronde 2 CLOSED build + P1 terbukti terperbaiki (8/8 probe, SIGINT 0,00 dtk); suite 259/259, GNU 5.501.128 byte, musl 5.633.200 byte, E2E 18/18. Detail: AUDIT_STATUS item 32, ROADMAP 13.2, workplan item 37. |
| **B3 — Inspector + subagent live** | Panel inspector (detail tool call, diff preview, Terapkan/Tolak), titik emisi `on_subagent_event` di task tool → bus → SSE (`SubagentSpawned`/`SubagentStatusChanged`), panel status Clowder di sidebar | B1, B2 | sedang: emisi event harus tidak mengubah urutan/audit turn |
| **B4 — Plugin hot-reload** | API `mount`/`unmount` di `purrcode-plugin` + endpoint + UI (daftar hook, config, mount/unmount tanpa restart gateway) | B2 | sedang: lifecycle proses plugin (spawn/kill bersih, tanpa bocor fd) |
| **B5 — Memory + Skill penuh** | POST save `MEMORY.md` (dengan validasi + audit), tampilkan fakta backend aktif; daftar SKILL.md penuh + tambah/edit dari UI (tetap lewat `manage_skill`, bukan tulis file langsung dari handler) | B2 | rendah-sedang: tulis file dari web = permukaan baru — butuh approval + audit seperti jalur lain |
| **B6 — Scheduler dari UI** | Tambah job dari UI (parser NL yang sama dengan CLI `cron add`), enable/disable/remove, tampilkan `next_run` | B2 (reuse `cron` store/queue yang sama) | rendah: reuse penuh jalur CLI; jangan duplikasi logika jadwal |
| **B7 — Mobile + dashboard** | Mobile drawer + layout responsif (F24: pakai dari HP); halaman overview "dashboard canvas" (kesehatan gateway, sesi aktif, cron berikutnya, delivery terakhir) — definisi di G11 | B1–B3 (butuh data sesi + event) | rendah-sedang: murni frontend; awas `html.rs` 713 → split dulu |
| **B8 — Keamanan F25** | Session timeout, token per-sesi (atau tetap satu token — putuskan di B8), rate limit login/prompt, audit semua aksi web, dokumentasi reverse-proxy (Caddy/nginx contoh) | B2 | sedang: jangan pecahkan token flow R3 yang sudah terverifikasi |
| **Close-out** | Dogfood live dari HP (F24: buka WebUI dari HP, kirim prompt, setujui kartu, cek delivery) + laporan akhir v0.4 (satu laporan, seperti v0.3) | semua batch | — |

**Verifikasi per batch** (sama seperti v0.3): suite + clippy `-D warnings` +
fmt bersih; E2E mock (harness baru per batch, port terpisah); 2 checker
independen diam-diam (build + adversarial), tree dibekukan selama verifikasi;
temuan checker jadi fix task, bukan laporan. Push per batch hanya atas
persetujuan per-aksi user.

## 6. Risiko & batasan

- **ADR-011:** `html.rs` 713 dan `server/mod.rs` 651 sudah dekat batas 800.
  Setiap batch yang menyentuh mereka harus split dulu (B2/B7 paling rawan).
  `loop_engine/mod.rs` 791 + `schedule/mod.rs` 790 juga tinggal 9–10 baris —
  jangan sentuh mereka kecuali perlu.
- **Ukuran binary:** Axum + dependensinya menambah ukuran; pagu 25 MB masih
  jauh (sekarang ~5,3 MB) tapi ukur tiap batch.
- **Musl static:** dependensi baru harus bisa static-pie (Axum bisa; bila ada
  crate yang tidak bisa, batalkan).
- **SSE replay:** replay via HTTP (D3) — jangan coba "replay SSE" (buffer
  history di bus tidak dirancang untuk itu).
- **Satu-satunya state live yang hilang:** tidak ada supervisor gateway
  (kematian diam-diam #1–#7 di v0.3). WebUI dari HP tidak berguna bila gateway
  mati — `service install` tetap deferred, dicatat di sini supaya tidak lupa.

## 7. Di luar scope v0.4 inti

- **D5** event stream `--json` penuh (sudah tidak diblokir R3, tapi bukan
  F23–F25 — opsi stretch setelah B8, bukan bagian definisi selesai).
- **D6** bash subshell persisten (opsi stretch, sama).
- **F3** failover top-level (butuh keputusan desain config — tetap deferred).
- **Install-approval plugin** (deferred sejak v0.1).
- **`service install`** (generate-only; supervisor gateway — deferred).
- **Tag v0.3** (ditunda atas keputusan user 2026-11-13).

## 8. Yang masih butuh jawaban user

1. **D1 — Axum, setuju?** (alternatif: tetap std+tokio — lebih sedikit
   dependensi, tapi routing manual makin mahal.)
2. **D2 — TLS built-in atau reverse-proxy-only?** (rekomendasi: proxy dulu.)
3. **D4 — batas sesi web bersamaan?** (rekomendasi: tanpa batas keras dulu.)
4. **B7 — dashboard canvas = halaman overview** seperti G11, atau ada isi
   lain yang kamu mau (grafik? log tail? usage token?).
5. **Urutan:** B1→B8 seperti di atas, atau ada yang mau didahulukan (mis.
   mobile dulu karena F24 HP)?
