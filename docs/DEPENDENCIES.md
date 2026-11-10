# PurrCode AI — Audit Lisensi Dependensi (DEPENDENCIES.md)

Status: Terverifikasi terhadap `Cargo.toml` workspace (v0.1.0).  
Lisensi Proyek: **GNU General Public License v3.0 or later (`GPL-3.0-or-later`)**.

Semua dependensi eksternal di bawah ini menggunakan lisensi permisif yang sepenuhnya kompatibel dengan distribusi biner statis di bawah **GPL-3.0**:

| Crate | Versi | Lisensi Upstream | Kompatibel GPL-3.0 | Kegunaan di PurrCode AI |
|---|---|---|---|---|
| `tokio` | `1.40` | MIT | ✅ Ya | Runtime async, process spawning, timer, channel |
| `reqwest` | `0.12` | MIT OR Apache-2.0 | ✅ Ya | Klien HTTP/SSE dengan `rustls-tls` (tanpa OpenSSL) |
| `futures-util` | `0.3` | MIT OR Apache-2.0 | ✅ Ya | Combinator stream async |
| `serde` | `1.0` | MIT OR Apache-2.0 | ✅ Ya | Serialisasi & deserialisasi struktur data |
| `serde_json` | `1.0` | MIT OR Apache-2.0 | ✅ Ya | Parser JSON untuk config, sesi JSONL, dan JSON-RPC |
| `thiserror` | `1.0` | MIT OR Apache-2.0 | ✅ Ya | Derivasi tipe error `PurrError` |
| `anyhow` | `1.0` | MIT OR Apache-2.0 | ✅ Ya | Konteks error ergonomis |
| `tracing` | `0.1` | MIT | ✅ Ya | Observabilitas & structured logging |
| `tracing-subscriber` | `0.3` | MIT | ✅ Ya | Filter log berbasis environment |
| `async-trait` | `0.1` | MIT OR Apache-2.0 | ✅ Ya | Trait async dinamis (`Tool`, `ModelProvider`, `MemoryBackend`, `Plugin`, `Channel`) |
| `chrono` | `0.4` | MIT OR Apache-2.0 | ✅ Ya | Timestamp RFC3339 pada audit log, sesi, dan handoff |
| `uuid` | `1.10` | MIT OR Apache-2.0 | ✅ Ya | Generator continuation ID (`purr_<id>`) & file sementara atomik |
| `clap` | `4.5` | MIT OR Apache-2.0 | ✅ Ya | Parser argumen & subcommand CLI (`purrcode-ai`) |
| `regex` | `1.10` | MIT OR Apache-2.0 | ✅ Ya | Pencarian teks `grep` & kompilasi pola `glob` |
| `walkdir` | `2.5` | Unlicense OR MIT | ✅ Ya | Traversal direktori rekursif (`find`, `glob`, `grep`) |
| `similar` | `2.6` | Apache-2.0 | ✅ Ya | Pembuatan unified diff preview pada `write` & `edit` |
| `ratatui` | `0.28` | MIT | ✅ Ya | Renderer TUI terminal |
| `crossterm` | `0.28` | MIT | ✅ Ya | Backend terminal lintas platform (raw mode & key events) |

## Aturan Penambahan Dependensi Baru
1. Dilarang menambahkan crate berlisensi proprietary, non-komersial (misal Sustainable Use License), atau AGPL yang bertentangan dengan kebijakan distribusi.
2. Gunakan `rustls` (bukan `native-tls` / OpenSSL) agar kompilasi statis musl tetap mandiri.
3. Setiap penambahan crate di `[workspace.dependencies]` wajib dicatat di tabel ini.
