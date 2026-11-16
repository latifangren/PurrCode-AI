# Desain Store SQLite PurrCode AI

> **Status:** DRAFT — spek desain, belum implementasi. Ditulis 2026-11-16 dari tree
> `dev-muse` @ `9dee491` (pasca-hapusan UI Rust lama).
> Keputusan mengikatnya dicatat sebagai **ADR-015** di `docs/DECISIONS.md`;
> dokumen ini adalah detailnya. Implementasi mengikuti pola batch v0.4
> (fmt/clippy, suite GNU+musl, E2E) dan push hanya atas persetujuan eksplisit
> per-aksi, seperti biasa.
>
> Aturan bahasa repo: kode Inggris, dokumen boleh Indonesia — dokumen ini
> Indonesia, identifier tetap Inggris.

## 1. Konteks

PurrCode hari ini **nol database**: tidak ada dependensi DB apa pun di
`Cargo.toml`/`Cargo.lock` (tanpa rusqlite, sqlite, sled, dsb). Semua state
mesin adalah flat file di `<purrcode_dir>` (`$HOME/.purrcode`,
`purrcode-service/src/lib.rs:96`):

| Store | Hari ini | Lokasi kode |
|---|---|---|
| Sesi (TUI/Web/Telegram/cron) | `sessions/<id>.jsonl`, satu file per sesi, tulis atomik tmp+rename | `crates/purrcode-core/src/session.rs:38` (`SessionStore`) |
| Cron jobs | `cron/jobs.jsonc`, load/save utuh tiap perubahan | `crates/purrcode-service/src/cron/store.rs:153` (`CronStore::load`), `:178` (`save`) |
| Delivery cron/heartbeat | `cron/deliveries.jsonl`, append-only | `crates/purrcode-service/src/cron/queue.rs:57` |
| Audit | `audit.jsonl`, append-only, rotasi di 5 MB | `crates/purrcode-approval/src/lib.rs:34` (`AuditLogger`) |
| Chat Telegram | `chats.json` (last active chat + kind) | `crates/purrcode-channel/src/telegram/sessions.rs:53` (`ChatSessions::FILE_NAME`) |

Itu cukup selama ini: satu user, append-mostly, file bisa dibaca manusia dan
di-grep, backup tinggal copy folder, tanpa migrasi dan tanpa daemon. Tapi
tiga hal mulai terasa:

1. **Query = full scan.** Kurator skill (F20) menghitung tool call dengan
   membaca *seluruh* `audit.jsonl` ke string
   (`crates/purrcode-approval/src/lib.rs:56`,
   `count_tool_calls_since_curator_check`); riwayat delivery dibaca dengan
   tail file mentah (`routes_overview.rs:131`); daftar sesi + judul + jumlah
   pesan butuh buka tiap file.
2. **Penulis bersamaan tanpa kunci.** Gateway, WebUI, TUI, dan CLI adalah
   proses terpisah yang menulis ke `purrcode_dir` yang sama
   (`SessionStore::new` dipanggil di `telegram/channel.rs:664`,
   `web/sessions.rs:187`, `cli/main.rs:152`, `gateway.rs:388`).
   Read-modify-write `jobs.jsonc` dan `chats.json` rawan tabrakan antar
   proses; JSONL append aman tapi tanpa indeks.
3. **Tumbuh tanpa batas, tanpa indeks.** `audit.jsonl` hanya dirotasi
   (5 MB), tidak ada cara murah bertanya "tool call sesi X sejak marker Y".

Keputusan ini **bukan hal baru**: `docs/ARCHITECTURE.md:64` sudah menulis
session store sebagai "JSONL append-only berversi + SQLite (`session`,
`turn`, `tool_call`)" (pola opencode SQLite+Drizzle, codex rollout JSONL,
pi session tree v3), dan `ARCHITECTURE.md:293` menyebut `sqlite-fts`
sebagai backend opsional. Spek ini **mengonfirmasi dan menyelesaikan**
arah itu: SQLite menjadi store mesin, JSONL ditinggalkan.

**Yang TIDAK berubah:** memori tetap Markdown (ADR-010) — `MEMORY.md` /
`USER.md` / `SOUL.md` adalah file milik user, bukan state mesin.

**Tanpa migrasi — keputusan user 2026-11-16:** repo masih dari 0, belum ada
data penting. Tidak ada kode impor dari JSONL/JSONC lama. File lama dibiarkan
di disk dan tidak dibaca lagi (user boleh hapus manual); DB baru dibuat
fresh saat pertama jalan.

## 2. Keputusan (ADR-015, ringkas)

1. **SQLite embedded, satu file per `purrcode_dir`:**
   `<purrcode_dir>/purrcode.db`, dibuat saat pertama dibuka, mode file `0600`
   (isinya transkrip sesi — perlakukan seperti kredensial). Tanpa server,
   tanpa daemon, tanpa jaringan. Binary tetap satu, gratis, local-first.
2. **Crate `rusqlite`, fitur `bundled`** (SQLite ikut ter-link statis —
   musl static tetap jalan). Versi exact di-pin saat implementasi dan dicatat
   di commit. Tanpa ORM, tanpa query builder: SQL mentah + `rusqlite`, supaya
   skema terlihat di satu tempat dan tidak ada magic.
3. **WAL + `busy_timeout`:** `PRAGMA journal_mode=WAL`,
   `PRAGMA synchronous=NORMAL`, `PRAGMA foreign_keys=ON`,
   `PRAGMA busy_timeout=5000`. WAL = pembaca bersamaan + satu penulis;
   timeout 5 detik menutupi tabrakan tulis gateway/Web/TUI/CLI.
4. **Satu koneksi per handle, di balik `Mutex`, semua call blocking lewat
   `tokio::task::spawn_blocking`.** Tidak ada pool (satu user, tulis jarang,
   pool hanya menambah kerumitan). Trait sinkron di dalam, async di batas.
5. **Skema berversi via `PRAGMA user_version` + tabel `schema_meta`.**
   Migrasi = nomor naik + statement DDL; tiap migrasi punya test yang
   membuat DB v(n-1), menjalankan migrasi, lalu assert bentuk + isi v(n).
6. **Satu store mesin untuk semua state mesin (§3).** Tidak ada dual backend,
   tidak ada feature flag file-vs-SQLite, tidak ada fallback senyap ke file.
   Yang gagal buka DB = hard error saat startup, bukan jatuh ke JSONL.
7. **Yang tetap file (§6) tidak pernah masuk DB.** Terutama kredensial:
   `~/.purrcode/credentials` tetap file `0600`, tidak pernah ditulis ke
   SQLite (DB bisa di-copy untuk debug; kredensial tidak boleh ikut).

## 3. Skema

Satu file, tujuh tabel. `TEXT` untuk id (format id yang sudah ada
dipertahankan: `purr_web`, `purr_cron_<id>_<run>`, `purr_hb_<hex>`,
`purr_<id>`, `cron_<12 hex>`). Waktu = RFC3339 `TEXT` (sama seperti yang
sudah dipakai di JSONL/JSONC — jangan campur epoch). JSON kolom = `TEXT`
berisi JSON valid, nama kolom berakhiran `_json`.

```sql
CREATE TABLE sessions (
  id          TEXT PRIMARY KEY,          -- purr_web, purr_web_<12 hex>, purr_cron_..., purr_hb_..., purr_<id>
  title       TEXT NOT NULL DEFAULT '',  -- judul = pesan user pertama (seperti list_sessions hari ini)
  parent_id   TEXT REFERENCES sessions(id), -- fork/branch: sesi anak menunjuk sesi sumber
  preset_json TEXT,                      -- snapshot preset sesi (opsional; NULL = default)
  created_at  TEXT NOT NULL,
  updated_at  TEXT NOT NULL
);
CREATE INDEX idx_sessions_updated ON sessions(updated_at DESC);

CREATE TABLE turns (
  id                INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id        TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  seq               INTEGER NOT NULL,    -- urutan pesan dalam sesi (1, 2, 3, ...)
  role              TEXT NOT NULL,       -- user | assistant | tool
  content           TEXT NOT NULL DEFAULT '',
  tool_call_id      TEXT,                -- pesan hasil tool: id call yang dijawabnya
  tool_calls_json   TEXT,                -- pesan assistant: daftar tool call (JSON)
  prompt_tokens     INTEGER,             -- terisi bila provider melaporkan usage; NULL = tak diketahui
  completion_tokens INTEGER,
  created_at        TEXT NOT NULL,
  UNIQUE (session_id, seq)
);
CREATE INDEX idx_turns_session_seq ON turns(session_id, seq);

CREATE TABLE tool_calls (
  id          TEXT PRIMARY KEY,          -- call_id dari provider (model-controlled)
  turn_id     INTEGER NOT NULL REFERENCES turns(id) ON DELETE CASCADE,
  session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  name        TEXT NOT NULL,             -- nama tool (read, bash, task, ...)
  args_json   TEXT NOT NULL DEFAULT '{}',
  status      TEXT NOT NULL DEFAULT 'done', -- done | error | denied
  created_at  TEXT NOT NULL
);
CREATE INDEX idx_tool_calls_session ON tool_calls(session_id, created_at);

CREATE TABLE cron_jobs (
  id               TEXT PRIMARY KEY,     -- cron_<12 hex>
  name             TEXT NOT NULL,
  enabled          INTEGER NOT NULL DEFAULT 1,
  schedule_expr    TEXT NOT NULL,        -- "0 * * * *"
  schedule_display TEXT NOT NULL,        -- "hourly" (echo NL parser, ADR-014 item 3)
  prompt           TEXT NOT NULL,
  tools_json       TEXT NOT NULL DEFAULT '[]',
  preset_json      TEXT NOT NULL,        -- snapshot preset saat job dibuat (ADR-014 item 4)
  delivery_json    TEXT NOT NULL,        -- {kind: queue_only|...}
  created_at       TEXT NOT NULL,
  last_run_at      TEXT,
  next_run_at      TEXT
);

CREATE TABLE deliveries (
  id           TEXT PRIMARY KEY,        -- dlv_<hex> (format yang sudah ada)
  job_id       TEXT REFERENCES cron_jobs(id) ON DELETE SET NULL, -- NULL = heartbeat / manual
  run_id       TEXT NOT NULL,           -- purr_cron_<id>_<run> / purr_hb_<hex>
  kind         TEXT NOT NULL,           -- cron | heartbeat
  status       TEXT NOT NULL,           -- claimed | delivered | failed
  detail_json  TEXT NOT NULL DEFAULT '{}',
  created_at   TEXT NOT NULL,
  delivered_at TEXT
);
CREATE INDEX idx_deliveries_job ON deliveries(job_id, created_at DESC);
CREATE INDEX idx_deliveries_status ON deliveries(status, created_at);

CREATE TABLE audit (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  ts         TEXT NOT NULL,
  session_id TEXT NOT NULL,
  agent      TEXT NOT NULL,              -- meowstro, tinker, tuxeshadow, web, ...
  event      TEXT NOT NULL,              -- tool_call, approval, shadow_verdict, ...
  tool       TEXT,                        -- nama tool; NULL bila event bukan tool
  decision   TEXT NOT NULL,              -- allow | deny | ask
  detail     TEXT NOT NULL DEFAULT ''
);
CREATE INDEX idx_audit_session_event ON audit(session_id, event, id);
CREATE INDEX idx_audit_ts ON audit(ts);

CREATE TABLE chats (
  chat_id        INTEGER PRIMARY KEY,    -- Telegram chat id
  kind           TEXT NOT NULL,          -- private | group | supergroup | channel
  last_active_at TEXT NOT NULL,          -- Unix seconds sebagai TEXT (format yang sudah ada)
  title          TEXT
);
```

Catatan skema:

- **`turns` + `tool_calls`, bukan satu tabel pesan:** `turns` menyimpan
  transkrip apa adanya (termasuk pesan hasil tool dengan `tool_call_id`);
  `tool_calls` menyimpan record call (nama + argumen + status) yang terikat
  ke turn assistant yang membuatnya. Ini yang memungkinkan "tool apa saja
  yang jalan di sesi ini" tanpa parse JSON — dan ini bentuk yang sama
  dengan tabel `session`/`turn`/`tool_call` di `ARCHITECTURE.md:64`.
- **`deliveries.status` menggantikan klaim file:** at-most-once ala Hermes
  (ADR-014 item 2) menjadi transaksi — `INSERT ... status='claimed'`,
  kirim, lalu `UPDATE ... status='delivered'` dalam satu transaksi.
  `claimed` tanpa `delivered` = dianggap terkirim, seperti sekarang.
- **`audit.id` monotonik** adalah pengganti "sejak marker terakhir":
  kurator cukup `SELECT COUNT(*) ... WHERE id > (marker)` — tanpa baca
  file utuh. Marker `skill_curator_checked` tetap dicatat sebagai event
  audit (bukan tabel terpisah).
- **Tidak ada tabel `messages` terpisah dari `turns`:** satu pesan = satu
  turn. Ringkasan compaction tetap disimpan sebagai pesan ringkasan di
  `turns` (seperti `compact_messages` hari ini — non-destruktif, JSONL asli
  tidak lagi relevan karena tidak ada JSONL).
- **Tidak ada FTS di v1.** Pencarian teks sesi/memory adalah follow-up
  (`sqlite-fts` sebagai plugin, `ARCHITECTURE.md:293`) — bukan bagian
  spek ini.

## 4. Aturan akses & konkurensi

- **Pembuka:** `Store::open(<purrcode_dir>)` → buka/buat
  `<purrcode_dir>/purrcode.db`, jalankan `PRAGMA` §2.3, jalankan migrasi
  yang tertunda, `chmod 0600` saat file baru dibuat. Gagal buka/migrasi =
  hard error startup (fail-closed, §2.6).
- **Satu koneksi per handle.** `Store` memegang `Mutex<Connection>`;
  semua metode sinkron; pemanggil async memakai `spawn_blocking`.
  Tidak ada koneksi per-thread (rusqlite `Connection` bukan `Send` tanpa
  `Send` feature — jangan nyalakan, itu menyembunyikan race).
- **Tulis dalam transaksi bila lebih dari satu statement**
  (claim+deliver, append turn + tool_calls, buat sesi + turn pertama).
  Satu statement = transaksi implisit SQLite, cukup.
- **Test memakai `:memory:`** lewat `Store::open_memory()` (konstruktor
  terpisah — jangan sulap path). Fixture DB temp file untuk test migrasi
  dan konkurensi multi-proses (dua proses tulis bergantian, assert tidak
  ada `database is locked` di luar `busy_timeout`).
- **Backup = copy file** (`purrcode.db` + checkpoint WAL dulu via
  `PRAGMA wal_checkpoint(TRUNCATE)` sebelum copy — catat sebagai command
  `purrcode-ai db backup` kandidat, bukan bagian v1).

## 5. Penempatan crate & API

- **Crate baru `purrcode-store`**, dependensi hanya `purrcode-api`
  (untuk `ChatMessage`, `AuditSink`) + `rusqlite` + `serde`/`serde_json`.
  Rantai satu arah tetap: `api → store → core → … → service → channel
  → cli`. Tidak ada dependensi balik.
- **Store lama jadi facade tipis** di crate-nya masing-masing, call site
  tidak berubah bentuk:
  - `SessionStore::new(dir)` tetap ada — isinya membuka
    `<dir>/purrcode.db` (bukan `sessions/`). API publiknya tetap
    (`append_message`, `load_messages`, `replace_messages`,
    `fork_session`, `delete_session`, `list_sessions`,
    `export_session`, `compact_messages`) — implementasi di dalamnya
    jadi SQL. `session_jsonl_path` / `sessions_dir` dihapus.
  - `CronStore::load(dir)` / `save()` tetap ada — isinya SQL, bukan
    `jobs.jsonc`. `jobs_file_path` / `cron_dir` untuk jobs dihapus
    (direktori `cron/` tetap dipakai? Tidak — `deliveries.jsonl` juga
    pindah; direktori `cron/` tidak lagi dibuat).
  - `AuditLogger::new(dir)` tetap ada — isinya SQL. Metode
    `count_tool_calls_since_curator_check` jadi satu query indeks
    (§3, catatan `audit.id`). `audit_file_path` dihapus.
  - Delivery queue (`cron/queue.rs`) — `claim`/`read_events` jadi SQL
    atas tabel `deliveries`. `deliveries_file_path` dihapus.
  - `ChatSessions` (`telegram/sessions.rs`) — `load`/`touch` jadi SQL
    atas tabel `chats`. `FILE_NAME` dihapus.
  - `read_last_delivery` (`routes_overview.rs:121`) jadi
    `SELECT ... FROM deliveries ORDER BY created_at DESC LIMIT 1`.
- **Tidak ada trait backend ganda.** Karena greenfield tanpa data penting,
  tidak ada `FileSessionBackend` vs `SqliteSessionBackend`, tidak ada
  feature flag. Satu implementasi, satu jalan. (Ini disengaja — dual
  backend adalah biaya pemeliharaan permanen untuk kompatibilitas yang
  tidak dibutuhkan.)
- **Audit trait tetap:** `AuditSink` (`purrcode-api/src/tool.rs:34`)
  tidak berubah; `AuditLogger` adalah implementornya yang kini SQL.
  `RecordingAuditSink` di test tetap (in-memory, bukan SQLite).

## 6. Yang tetap file (tidak masuk DB)

| Tetap file | Alasan |
|---|---|
| `MEMORY.md` / `USER.md` / `SOUL.md` + `.purrcode/MEMORY.md` (ADR-010) | Milik user, dibaca/diedit manusia, di-commit ke git |
| Skills (`~/.purrcode/skills/`, `.purrcode/skills/`) | Markdown + frontmatter, discovery berbasis file |
| Config `purrcode.yaml`/`purrcode.jsonc`, presets, model-presets | Milik user, human-editable; config baru tidak butuh section store (path DB diturunkan dari `purrcode_dir`, bukan config) |
| `~/.purrcode/credentials` (0600) | Secret tidak pernah masuk DB (§2.7) |
| Plugin manifest + file plugin | Discovery berbasis file |
| `HEARTBEAT.md`, `PROACTIVE_PREFERENCES.md` | Dokumen milik user |

## 7. Yang dihapus (tanpa migrasi)

File yang tidak lagi dibaca/dibuat — dihapus kodenya, file lamanya
dibiarkan di disk (tidak dihapus otomatis, tidak diimpor):

- `sessions/<id>.jsonl` + `sessions/purr_<id>/` (seluruh `SessionStore`
  file impl — `session.rs` ditulis ulang ke SQL)
- `cron/jobs.jsonc` + `cron/deliveries.jsonl` (seluruh `CronStore` file
  impl + `cron/queue.rs` JSONL)
- `audit.jsonl` + rotasi `audit-<tanggal>.jsonl.gz` (seluruh
  `AuditLogger` file impl; rotasi diganti retensi SQL — lihat §10)
- `chats.json` (`ChatSessions` file impl)
- Jalur baca file di WebUI: `read_last_delivery`
  (`routes_overview.rs:121`), `get_audit_logs` dari `audit.jsonl`
  (`routes_cockpit.rs:36`) → SQL.

**Tanpa kode impor.** Tidak ada `db import`, tidak ada konversi satu
kali, tidak ada mode kompatibilitas. Alasan: keputusan user 2026-11-16 —
masih dari 0, belum ada data penting. Menulis importer untuk data yang
tidak penting adalah kode yang langsung jadi dead code.

## 8. Query kunci (acceptance)

Spek ini diterima bila query berikut masing-masing satu statement SQL
(atau satu transaksi bila tertulis demikian):

```sql
-- Daftar sesi + judul + jumlah pesan (WebUI /api/sessions, TUI)
SELECT s.id, s.title, s.updated_at, COUNT(t.id) AS message_count
FROM sessions s LEFT JOIN turns t ON t.session_id = s.id
GROUP BY s.id ORDER BY s.updated_at DESC;

-- Transkrip satu sesi (resume)
SELECT role, content, tool_call_id, tool_calls_json
FROM turns WHERE session_id = ? ORDER BY seq;

-- Fork sesi (salin turn + tool_calls, parent_id = sumber)
-- transaksi: INSERT INTO sessions (id baru, parent_id = sumber, ...)
--            + INSERT INTO turns SELECT ... + INSERT INTO tool_calls SELECT ...

-- Kurator F20: tool call sejak marker terakhir (tanpa full scan)
SELECT COUNT(*) FROM audit
WHERE session_id = ? AND event = 'tool_call'
  AND id > COALESCE(
    (SELECT MAX(id) FROM audit
     WHERE session_id = ? AND event = 'skill_curator_checked'), 0);

-- Overview B7: sesi aktif + next cron + delivery terakhir
SELECT COUNT(*) FROM sessions;
SELECT * FROM cron_jobs WHERE enabled = 1 AND next_run_at IS NOT NULL
  ORDER BY next_run_at LIMIT 1;
SELECT * FROM deliveries ORDER BY created_at DESC LIMIT 1;

-- Cron due scan (runner): job enabled yang jatuh tempo
SELECT * FROM cron_jobs WHERE enabled = 1 AND next_run_at <= ?;

-- Delivery claim at-most-once (transaksi)
INSERT INTO deliveries (id, job_id, run_id, kind, status, created_at)
VALUES (?, ?, ?, ?, 'claimed', ?);
-- ... kirim ...
UPDATE deliveries SET status = 'delivered', delivered_at = ?
WHERE id = ? AND status = 'claimed';
```

## 9. Fase implementasi (urutan)

1. **P1 — Fondasi:** crate `purrcode-store` + `rusqlite(bundled)` +
   `Store::open`/`open_memory` + `PRAGMA` §2.3 + migrasi v1 (skema §3)
   + test migrasi + test konkurensi dua proses.
2. **P2 — Sesi:** `SessionStore` jadi SQL (§5). Seluruh test sesi
   yang ada harus hijau tanpa diubah perilakunya (transkrip, fork,
   compaction, resume `purr_<id>`).
3. **P3 — Cron + delivery:** `CronStore` + queue jadi SQL; klaim
   transaksi (§8); `cron` CLI + WebUI scheduler tidak berubah perilaku.
4. **P4 — Audit:** `AuditLogger` jadi SQL; `count_tool_calls_since_
   curator_check` jadi query indeks; audit WebUI (`/api/audit`,
   `/api/logs`) baca SQL. Test: kurator tetap trigger di ≥5 tool call.
5. **P5 — Chat Telegram:** `ChatSessions` jadi SQL; `last_active_private_
   chat` + fallback `allow_from[0]` tidak berubah.
6. **P6 — Hapus jejak file:** hapus semua kode baca/tulis JSONL/JSONC
   §7 + test yang mengassert isi file (ganti assert SQL); `cargo fmt` +
   `cargo clippy --workspace --all-targets -- -D warnings`; suite
   GNU+musl; E2E 108/108 (driver tidak berubah — mock + temp workspace
   tetap jalan, DB-nya file temp); laporan akhir milestone.

Estimasi: **satu milestone** (diusulkan v0.5 — penomoran diputuskan
saat mulai, bukan di spek ini). Bukan batch kecil: menyentuh 5 store
sekaligus, dan P2–P5 masing-masing butuh checker sendiri seperti pola
v0.4 (freeze tree → checker → fix → verifikasi).

## 10. Risiko & non-goals

- **Retensi audit:** tanpa rotasi file, tabel `audit` tumbuh tanpa
  batas. v1: `DELETE FROM audit WHERE ts < date('now','-90 days')`
  dijalan sekali saat startup (90 hari — sama dengan retensi minimal
  30 hari yang direncanakan di `ARCHITECTURE.md:496`, dibulatkan ke
  atas). Tanpa vacuum otomatis v1.
- **Ukuran binary:** `rusqlite(bundled)` menambah ±1–2 MB — masih jauh
  di bawah target ≤25 MB (rilis kini ~6,5 MB).
- **Startup:** buka DB + migrasi < 50 ms untuk DB kosong; tidak ada
  server yang di-start.
- **Corrupt DB:** satu file = satu titik gagal (sebelumnya satu file
  rusak hanya merusak satu sesi/job). Mitigasi v1: `PRAGMA integrity_check`
  saat open gagal → hard error dengan pesan yang menyebut path file +
  cara backup (copy file) — bukan silent fallback. Backup otomatis
  terjadwal = follow-up, bukan v1.
- **Non-goals:** FTS5/pencarian teks (plugin, `ARCHITECTURE.md:293`);
  enkripsi at-rest (file 0600 sudah cukup untuk single-user lokal);
  sync/replikasi/multi-user; Postgres; export/import antar mesin
  (backup = copy file, §4).
- **Keputusan yang masih terbuka (diputuskan saat implementasi, bukan
  di spek ini):** versi exact `rusqlite`; apakah perlu subcommand
  `purrcode-ai db` (inspect/backup) atau cukup copy file manual.

## 11. Jejak keputusan

- `docs/ARCHITECTURE.md:64` — session store "JSONL + SQLite
  (`session`, `turn`, `tool_call`)" → dikonfirmasi, JSONL ditinggalkan.
- `docs/ARCHITECTURE.md:293` — `sqlite-fts` sebagai plugin → tetap
  follow-up, bukan v1.
- ADR-010 (Memori Default Markdown Minimal) → tidak berubah; §6.
- ADR-014 item 2 (cron store `jobs.jsonc` + `deliveries.jsonl`) →
  **diganti** oleh spek ini untuk store mesin (skema §3); semantik
  at-most-once dan safety item 4 ADR-014 tidak berubah.
- Setelah implementasi mendarat, `docs/ARCHITECTURE.md` §2 (tabel
  komponen + alur turn) diperbarui: "JSONL + SQLite sesi" → "SQLite
  sesi", "files (SQLite/JSONL/MD)" → "files (SQLite/MD)". Sengaja
  TIDAK diubah di spek ini — dokumen menggambarkan tree yang ada
  sampai kodenya berubah.
