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

## P1 — Berikutnya

| # | Item | Catatan |
|---|---|---|
| 8 | **TOCTOU SSRF — pin IP hasil resolusi saat connect** | `webfetch.rs` tidak ada pinning; `to_socket_addrs` 0 kemunculan di `crates/`; check vs connect memakai lookup DNS terpisah. Opsi: custom resolver/connector di reqwest yang memakai IP hasil validasi. Sambil jalan: dokumentasikan fetch publik tetap terblokir di VM proxy Muse (198.18.0.0/15) sebagai keterbatasan lingkungan, bukan "false positive teratasi". |
| 9 | Tegakkan `capabilities.network`/`filesystem` atau hapus klaimnya | Saat ini hanya `capabilities.secrets` yang ditegakkan (`scrub_plugin_env`, `crates/purrcode-plugin/src/lib.rs:149`). Field network/filesystem di-parse tapi 0 referensi produksi; plugin proses berjalan dengan hak penuh user. |
| 10 | Header `## Summary` ganda di `result.md` | Template + ringkasan mock sama-sama menulisnya (pre-existing, kosmetik). |
| 11 | Kartu hasil tool TUI hanya menampilkan baris pertama | `crates/purrcode-channel/src/tui/render.rs:309` (`output.lines().next()`, by-design). Pertimbangkan beberapa baris pertama + penanda "N baris lainnya" agar marker truncasi / baris match grep terlihat. |
| 12 | Multi-line fuzzy edit menghapus trailing newline | Artefak: baris berikut menyatu ke baris yang diganti (file tetap valid). Butuh boundary unit test. |
| 13 | **Musl build sungguhan** | `cargo zigbuild/cross` + `Cross.toml` + `.github/workflows` + target musl terinstal; ukur binary rilis musl. Sampai ada, "single static musl binary" tetap target, bukan fakta. |
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
