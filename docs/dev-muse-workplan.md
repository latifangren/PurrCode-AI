# Workplan `dev-muse` — PurrCode AI

**Branch:** `dev-muse` (dari `main@c56d3b7`, 2026-11-12)
**Pemilik branch:** Skyla AI / Muse Spark — mulai ronde ini saya juga **implementor** di branch ini (sebelumnya hanya verifier/auditor read-only). **Verifikasi tetap independen:** setiap commit di branch ini tetap diperiksa oleh dua pemeriksa independen (subagent segar) seperti ronde-ronde sebelumnya, dengan laporan di `~/workspace/your_files/purrcode-ai-verifikasi-*.md`.
**Aturan branch:** `main` tetap stabil; kerja di `dev-muse`; **push hanya dengan persetujuan eksplisit user per aksi** (berlaku juga untuk commit dokumentasi saja); tidak ada commit langsung ke `main`.

## Status awal (dari verifikasi `c56d3b7`, 2026-11-12 03:20 WIB)

- Suite: **75/76** (1 gagal: `test_resolve_windows_shell_detects_path_bash`).
- Binary rilis terukur: **4.612.528 byte = 4,40 MiB**, dynamic GNU, stripped (bukan 3,7 MB, bukan musl).
- Tertutup live: model sesi deterministik, polish kartu approval TUI (hint + `y` bekerja), dry-run ganda, registry `failed`, child `-w`, failover live, Strict preview, kartu TUI.
- Terbuka: TOCTOU SSRF, `src/` kosong, klaim dokumen (lihat P0).

## P0 — Ronde ini (branch `dev-muse`)

| # | Item | Bukti (file:line) | Kriteria selesai |
|---|---|---|---|
| 1 | Test PATH Windows portabel | `crates/purrcode-tools/src/execution/bash.rs:176-185` — fix memecah `;` tapi `entry.join("bash.exe")` di Linux menyisipkan `/` sehingga `C:\msys64\usr\bin/bash.exe` ≠ `C:\msys64\usr\bin\bash.exe`. Perbaikan: bangun path kandidat dengan pemisah `\` eksplisit untuk path bergaya Windows (helper `windows_join`), tambah unit test untuk helper. | `test_resolve_windows_shell_detects_path_bash` lolos di Linux; 2 test Windows lain tetap lolos; suite hijau. |
| 2 | Luruskan klaim angka dokumen | `docs/AUDIT_STATUS.md:7,47,157,174` ("76 lolos, 0 failed" / "100% passing" / "76/76 hijau") — tulis angka hasil ukur sendiri, bukan klaim. `:36,:41` "3.7 MB" telanjang → kualifikasi `.exe` Windows (belum terverifikasi) / `~4,4 MiB` Linux terukur. `:5,:167` angka byte eksak 4.604.336 → ukur ulang setelah build rilis branch ini (atau pakai `~4,4 MiB` saja). `:41` "7 bahasa" → 6. `:168` + `ROADMAP:100` "Bulletproof" → hapus kata itu sampai ada bukti uji Windows. `ROADMAP:3` "76 tests passing" → sesuaikan hasil ukur. | Semua angka di dokumen cocok dengan hasil `cargo test` + `cargo build --release` yang dijalankan di branch ini. |
| 3 | Hapus `src/` kosong | Root `src/`, 0 entri, mtime 10 Nov, tidak ter-track git. | Direktori hilang. |
| 4 | Unit test pemetaan tombol approval TUI | `crates/purrcode-channel/src/tui/mod.rs:121-143` — `y`/`a`/`Enter`/`t`/`n`/`Esc`/`e` inline di `run_loop`, tanpa unit test. Ekstrak fungsi murni `approval_key_action(code) -> Option<...>` + `#[cfg(test)]`. | Test baru lolos; perilaku live tidak berubah. |
| 5 | Deterministik langkah-3 `resolve_model` | `crates/purrcode-config/src/model.rs:134` — pencarian model telanjang masih mengiterasi `HashMap` providers. Urutkan nama provider sebelum iterasi + `#[cfg(test)]` baru. | 2 provider berbagi key model yang sama → hasil deterministik (provider leksikografis terkecil). |
| 6 | Teks musl + install-approval sebagai fakta | `AGENTS.md:15` ("single static musl binary"), `CLAUDE.md:32` (idem), `AGENTS.md:125` ("user approved it at install" — mekanisme tidak ada). Ubah jadi **target** (musl) dan **scope v0.3** (install-approval). Keputusan musl sebagai target TIDAK berubah (ARCHITECTURE/PRD/DECISIONS ADR-001). | Teks tidak lagi menyatakan keadaan yang belum ada. |
| 7 | "~140 MB" Weilin0723 | `docs/00-analysis-references.md:31` — ukuran tidak terverifikasi (tanpa clone lokal). | Ganti jadi "tidak terverifikasi". |

**Kriteria keluar P0:** `cargo check` lolos 0 warning, `cargo test --workspace` hijau (76/76 atau lebih bila ada test baru), `cargo clippy --workspace --all-targets -- -D warnings` lolos, `cargo fmt --check` bersih, `cargo build --release` + ukuran binary tercatat di `AUDIT_STATUS.md` §4 item 9 dengan nomor commit branch ini.

## Magpie dev-testing provider (2026-11-12)

Untuk pengujian development, PurrCode memakai model **`antigravity/gemini-3.8-flash-tiered`** dari gateway Magpie milik user di PC-nya (Cloudflare tunnel `https://magpie.raevtar.dev/v1`, 66 model). Terverifikasi live dari VM ini: completion 200 ("OK", 7,8 dtk), tool call 200 (`calc` a=2 b=2, 20,5 dtk). Lebih cepat dari `group/cheap-first` (22,7 dtk) — tetap lambat per langkah (model reasoning + tunnel/proxy).

- Wiring: provider `magpie` di `purrcode.jsonc.example` (base_url literal — endpoint publik milik user; `"api_key": "${MAGPIE_API_KEY}"` dari env saja, jangan pernah tulis key ke repo) + preset `model-presets/magpie.jsonc` (semua role → model yang sama) + embedded di `crates/purrcode-config/src/preset_model.rs` + assert di `test_bundled_model_presets_load`.
- Cara pakai: `export MAGPIE_API_KEY='<key>'` lalu `purrcode-ai -m magpie:antigravity/gemini-3.8-flash-tiered -p "..."` dengan `model_preset: "magpie"` di config (atau `-m` eksplisit untuk sesi). Sejak commit ronde autonomous 2026-11-12, `--orchestrator` adalah default (Meowstro + The Clowder); `--direct` memakai agent generik semua-tool sebagai opt-out.
- Terverifikasi live dari VM ini (2026-11-12): completion "OK" 7,8 dtk, tool call `calc` 20,5 dtk, binary `-p` baca file 25,5 dtk, delegasi `--orchestrator --yolo` end-to-end ~157 dtk (Meowstro `task` → Tinker `write` + `read` + `bash od` verifikasi).
- **Keterbatasan lingkungan:** binary membuat HTTPS langsung; VM ini hanya keluar lewat proxy egress → tes dari VM memakai bridge lokal `127.0.0.1:3457` (passthrough `/tmp/magpie_bridge.py`, bukan bagian repo). Di PC user binary langsung nyambung.
- **Temuan baru (terbuka):** (a) UA default `reqwest` di-block Cloudflare gateway ini (403, error 1010) — **koreksi 2026-11-12:** binary lama ternyata TIDAK mengirim header User-Agent sama sekali (reqwest 0.12 `Client::new()` tidak menyuntik UA); WAF kemungkinan menolak request tanpa UA itu, bukan literal `reqwest/*`. Default jujur PurrCode (`PurrCode-AI/0.1 ...`) lolos live (Tes D, tanpa `user_agent` → 200). Opsi `user_agent` tetap ada untuk gateway yang lebih ketat. (b) **`-p` tanpa `--orchestrator` bukan Meowstro** — agent generik + semua tool, audit tetap melabeli "meowstro" (`main.rs:816`); jalur read-only Meowstro hanya aktif dengan flag itu. Pengujian delegasi wajib pakai `--orchestrator`.
- Hanya model itu yang dites live; 65 model gateway lain belum dites satu-satu. Context window 1.048.576 adalah klaim gateway.

## Ronde autonomous (2026-11-12 04:16 WIB — perintah user)

User memerintahkan (04:16 WIB): kerjakan keduanya (default `--orchestrator` + User-Agent), full autonomous, sampai benar-benar jalan dan normal semua sesuai PRD, pakai referensi proyek lain, jangan ngarang/nebak, clone repo referensi bila perlu, utamakan open source (OMP, Hermes Agent, dll).

- **Scope: PRD v0.1 (F1–F14) + dua fix yang disetujui.** v0.2 (Telegram F15–F18), v0.3 (cron F19, sandbox OS F21, install-approval), v0.4 (WebUI F23–F25) di luar scope ronde ini — tetap didokumentasikan jujur sebagai belum ada.
- **Aturan:** setiap klaim fungsi harus diuji live atau unit test; pola dari referensi dikutip dengan file:line, bukan tebakan; commit lokal di `dev-muse` per batch, verifikasi independen 2 checker per batch; push tetap butuh persetujuan eksplisit per aksi.
- **Referensi:** 13 repo sudah ter-clone di `references/` (opencode, codex, oh-my-pi, pi, slim, oh-my-openagent, deepseek-harness, hermes-agent, openhuman, picoclaw, zeroclaw, Muse); riset pola UA/orchestrator/SSRF/capabilities dijalankan subagent riset read-only (hasilnya jadi masukan implementasi, bukan pengganti baca kode sendiri).
- **Item ronde ini:**

| # | Item | Catatan |
|---|---|---|
| 15 | `--orchestrator` jadi default (`-p` + TUI), `--direct` opt-out, label audit jujur | PRD §4.5.6 menuntut penegakan harness-level: Meowstro read-only + delegasi. Sebelum fix ini default `-p` menjalankan agent generik semua-tool yang salah dilabeli "meowstro". `--orchestrator` tetap diterima (eksplisit, tanpa efek tambahan); `--orchestrator` menang bila keduanya dikasih. |
| 16 | User-Agent configurable di provider config + default jujur | `ProviderConfig.user_agent` (opsional); bila kosong → default `PurrCode-AI/0.1 (purrcode-ai; https://github.com/latifangren/PurrCode-AI)`. **Hasil live 2026-11-12 (koreksi temuan awal):** binary lama TIDAK mengirim header User-Agent sama sekali (reqwest 0.12 `Client::new()` tidak menyuntik UA) dan Cloudflare gateway ini menolak request tanpa UA itu (403, error 1010); default jujur PurrCode **lolos** (Tes D: tanpa `user_agent` → 200). Jadi provider `magpie` di example TIDAK memakai UA browser — default jujur sudah cukup; opsi `user_agent` tetap ada untuk gateway yang lebih ketat. `headers["User-Agent"]` per-request tetap menang atas default client bila diset. Batch 2a: UA tidak valid di config divalidasi dulu — bila invalid → kembali ke `DEFAULT_USER_AGENT`, tidak pernah ke client tanpa UA. Fallback client polosnya dihapus sepenuhnya di item 21 (konstruktor fallible). |
| 17 | TOCTOU SSRF — pin IP hasil resolusi saat connect | P1 item 8, naik ke ronde ini. Lihat P1 untuk detail. |
| 18 | Tegakkan `capabilities.network`/`filesystem` atau hapus klaimnya | P1 item 9, naik ke ronde ini. **SELESAI batch 2a, sisi "hapus klaimnya":** hanya `secrets` ditegakkan; `network`/`filesystem` di-parse + dicatat sebagai deklarasi v0.3, peringatan via `tracing::warn!` + audit entry `plugin_capability_unenforced` + baris `⚠` di `plugin list`; klaim default-deny dikualifikasi. Penegakan nyata = sandbox OS v0.3. |
| 19 | Light fixes (result.md `##` ganda, fuzzy-edit newline, TUI first-line) | P1 item 10–12. **SELESAI batch 2a:** dedup `## Summary`, `line_content_end` (LF + CRLF), kartu TUI 4 baris + marker `… (N more lines)` — semua ber-unit-test. |
| 20 | Musl build sungguhan + CI workflow | **SELESAI 2026-11-12 05:08 WIB:** build musl 4.756.640 byte = 4,54 MiB, `static-pie linked`, stripped, `ldd` statically linked, suite musl 98/98; CI `.github/workflows/ci.yml` 6 job (YAML divalidasi lokal, belum pernah jalan di GitHub — job Windows belum terverifikasi). Tidak memakai zigbuild/cross. |
| 21 | Temuan checker batch 2a (F1/F2/N1b) + SSE parser | **SELESAI batch 2b (2026-11-12 05:02 WIB):** F1 header §3 orchestrator 7→8; F2 `CliApprovalRequester { yolo }` + hint jujur + unit test; N1b `new()`/`from_config()` fallible, fallback `Client::new()` (panic trap) dihapus; SSE `index` `#[serde(default)]` + `tracing::warn!` untuk chunk malformed. Suite 98/98, release 4.629.616 byte. |
| 22 | Failover top-level (F3, TERBUKA) | `FallbackRouter` hanya di loop child; model sesi 500 → hard error by design. Butuh keputusan desain: dari mana rantai fallback model sesi berasal (tidak ada field `fallback` di `ModelConfig`). Bukan diklaim tertutup. |
| 23 | Dogfood v0.1 + perbaikan temuan D2/D3 | **SELESAI 2026-11-12 05:41 WIB:** TUI mdcount 21/21 + clippy 0 + JSON persis; `-p` --words-only 26/26 + flag benar (run dihentikan di loop PATH harness, implementasi sudah terbukti). D2: `[A] Always (session)` terverifikasi live (1 keypress, 4 aksi). D3: child cap 15→25 + hasil task record-only. TERBUKA: D5 `--json` delta-teks saja, D6 bash stateless. Suite 99/99, release 4.630.000 byte. |
| 24 | Batch 2d: perbaikan temuan verifikasi batch 2c | **SELESAI 2026-11-12 05:52 WIB:** F1 build checker — dead-model fallback diperbaiki (`spec.model` → `self.model`, regression test `test_unresolved_role_model_falls_back_to_session_model`); F1–F3 re-checker (dokumen: angka musl 100/100, anchor D5 :902, gloss §3) diperbaiki. Verifikasi batch 2c: 3/3 klaim PASS live, tanpa regresi, §3 eksak, dogfood 26/26, musl 100/100. Suite 100/100, release 4.630.000 byte, musl 4.756.640 byte. |

**Kriteria keluar ronde:** semua item 15–21 selesai dengan bukti ukur (unit test + live test bila memungkinkan dari VM ini), suite hijau, `AUDIT_STATUS.md` diperbarui per batch, dan verifikasi independen 2 checker menutup batch (item 22 = F3 failover top-level dicatat terbuka, butuh keputusan desain).

**Perintah user 2026-11-12 04:54 WIB:** beresin sampai benar-benar beres seperti coding tools pada umumnya, BUKAN bikin laporan satu per satu — verifikasi per batch tetap jalan diam-diam (temuan checker jadi daftar perbaikan langsung, tanpa laporan gabungan per batch). Sandboxing (F21) TIDAK prioritas — fitur akhiran; fokus = semua berfungsi normal seperti repo Hermes Agent. Urutan: v0.1 sampai benar-benar done (musl statis + CI, dogfood end-to-end proyek sungguhan) → v0.2 Telegram (F15–F18, ala Hermes; uji live butuh token bot dari user) → belakangan cron/WebUI/sandbox. Tetap di branch `dev-muse`, main tidak disentuh, push tetap butuh persetujuan eksplisit.

**Hasil riset referensi (subagent read-only, 2026-11-12 04:19 WIB)** — pola dari clone lokal `references/` (codex, opencode, oh-my-pi, pi, slim, oh-my-openagent, deepseek-harness, hermes-agent, openhuman, picoclaw, zeroclaw; lisensi dicatat, pola saja, bukan salinan kode):
- **Q1 User-Agent:** pola pi/codex — satu `user_agent()` bersama yang tak bisa dimatikan (`pi/<ver> (<platform>; <runtime>; <arch>)`, `codex/<ver>`) + override config ala picoclaw (`user_agent` key, default `PicoClaw/<Version>`). Fix 16 mengikuti pola ini (default jujur `PurrCode-AI/0.1 ...` + `user_agent` config). Hermes mencatat WAF yang menolak UA sebagai failure mode yang dikenal (`auxiliary_client.py:888`).
- **Q2 default one-shot:** norma industri UNANIM — single general agent sebagai default one-shot; orchestrator hanya opt-in (flag/config, ala opencode `--agent` / slim `default_agent`). **PurrCode sengaja menyimpang dari norma ini atas keputusan eksplisit user (04:16 WIB) + PRD §1/§4.5.6 yang mengikat** (Meowstro adalah arsitektur intinya, bukan mode). Bagian yang disepakati norma: label audit harus jujur + kedua mode didokumentasikan — keduanya sudah dikerjakan di fix 15.
- **Q3 SSRF pinning:** zeroclaw adalah pola acuan — `to_socket_addrs()` → validasi semua IP hasil resolve (non-global incl. `100.64/10`, `198.18/15`, semua `169.254/16`) → pin dengan reqwest `.resolve_to_addrs(host, addrs)` + `.no_proxy()` → redirect same-host ≤10 dengan re-validasi per hop. Item 17 memakai pola ini.
- **Q4 capabilities:** zeroclaw — `egress_hosts` allowlist per plugin + deny-by-default di boundary dispatch (`egress_host_matches`); opencode — `Rule { action, resource, effect }` allow/deny/ask, last-match-wins, fallback deny-all. Item 18 memakai gabungan ini (tanpa WASM — PurrCode tidak punya plugin host WASM).

## P1 — Berikutnya

| # | Item | Catatan |
|---|---|---|
| 8 | **TOCTOU SSRF — pin IP hasil resolusi saat connect** | `webfetch.rs` tidak ada pinning; `to_socket_addrs` 0 kemunculan di `crates/`; check vs connect memakai lookup DNS terpisah. Opsi: custom resolver/connector di reqwest yang memakai IP hasil validasi. Sambil jalan: dokumentasikan fetch publik tetap terblokir di VM proxy Muse (198.18.0.0/15) sebagai keterbatasan lingkungan, bukan "false positive teratasi". |
| 9 | Tegakkan `capabilities.network`/`filesystem` atau hapus klaimnya | Saat ini hanya `capabilities.secrets` yang ditegakkan (`scrub_plugin_env`, `crates/purrcode-plugin/src/lib.rs:149`). Field network/filesystem di-parse tapi 0 referensi produksi; plugin proses berjalan dengan hak penuh user. |
| 10 | Header `## Summary` ganda di `result.md` | Template + ringkasan mock sama-sama menulisnya (pre-existing, kosmetik). |
| 11 | Kartu hasil tool TUI hanya menampilkan baris pertama | `crates/purrcode-channel/src/tui/render.rs:309` (`output.lines().next()`, by-design). Pertimbangkan beberapa baris pertama + penanda "N baris lainnya" agar marker truncasi / baris match grep terlihat. |
| 12 | Multi-line fuzzy edit menghapus trailing newline | Artefak: baris berikut menyatu ke baris yang diganti (file tetap valid). Butuh boundary unit test. |
| 13 | **Musl build sungguhan** | **SELESAI (lihat item 20 ronde autonomous):** build musl terverifikasi 4.756.640 byte static-pie linked + suite musl 98/98 + CI 6 job. `zigbuild`/`cross` tidak diperlukan — `musl-gcc` cukup. |
| 14 | **Uji Windows sungguhan** | Butuh mesin Windows / CI Windows: deteksi Git Bash, codepage 65001, Scheduled Task, dan verifikasi ukuran `.exe` 3,7 MB. |

## P2 — Scope v0.3 (sudah jujur didokumentasikan)

- Cron scheduler daemon (belum ada scheduler di mana pun).
- Alur konfirmasi interaktif instalasi plugin (install-approval).
- Isolasi sandbox OS (cgroup/namespace).

## Catatan verifikasi yang tetap berlaku

- Model sesi deterministik memakai prioritas substring `contains("primary")` — deterministik, semantiknya longgar (`not-primary` ikut cocok).
- Kartu approval TUI: tombol `Enter`/`n` baru level kode (satu match arm dengan `a`/`y` dan `t`); belum diuji live.
- Meowstro `-w` belum terobservasi langsung live (hasil `read_skill` tidak dipersist di mode one-shot); child `-w` terverifikasi live.
- Approval crate paling tipis (2 test).
