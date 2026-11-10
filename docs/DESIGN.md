# PurrCode AI — Design (DESIGN.md)

Versi: 0.3 (revisi tinjauan ulang) — 2026-11-11
Cakupan: TUI terminal (v0.1) + Telegram (v0.2) + WebUI (v0.4) + config/onboarding
+ gaya repo. **Tugas saat ini hanya dokumentasi & arsitektur — bukan kode.**
Bahasa implementasi: Rust. Lisensi: GPL-3.0.

## 1. Prinsip desain

1. **Terminal dulu, terminal yang jujur.** TUI menampilkan apa yang sebenarnya
   terjadi: tool call, file yang akan diubah, token yang dipakai. Tidak ada
   animasi yang menyembunyikan kerja (pelajaran dari TUI omp: card tool call +
   preview edit sebelum tulis).
2. **Tanya saat ambigu, jangan tebak.** Bila instruksi ambigu, agen memanggil
   tool `ask` — picker opsi terstruktur di tengah turn — bukan menebak lalu
   merusak file. Ini perilaku default, bukan pengecualian.
3. **Satu baris, satu kebenaran.** Status subagent, token, model, dan preset
   yang aktif (mode + model) selalu terlihat di status bar; tidak ada state
   tersembunyi.
4. **Ringan secara visual.** Tanpa dashboard web, tanpa Electron, tanpa emoji
   berlebihan. Motif kucing (🐾) dipakai hemat: nama, prompt `purr>`, dan
   sesekali di pesan sukses — bukan di setiap baris.
5. **Bahasa mengikuti user, bukan dikunci.** Lihat §5 — aturan bahasa balasan
   adaptif (keputusan user 2026-11-11).

## 2. TUI (v0.1) — layout

```
┌─ PurrCode AI ── ollama:qwen3:8b ── tabby · free ── sesi: abc123 ── 12.4k/64k ─┐
│                                                                           │
│  > perbaiki bug login yang crash saat password kosong                     │
│                                                                           │
│  ◈ Whisker — mencari handler login…                       [running]      │
│    └─ grep "login" di src/ → 6 file                                       │
│  ◈ Sphinx — penyebab: nil deref di auth.rs:42               [done]         │
│  ◈ Tinker — mengusulkan patch                               [waiting]     │
│                                                                           │
│  ┌─ Preview: src/auth.rs ────────────────────────────────────────────┐    │
│  │ - if pw.is_empty() { return err }                                 │    │
│  │ + if pw.is_empty() { return Err(EmptyPassword) }                  │    │
│  └───────────────────────────────────────────────────────────────────┘    │
│  [a] Terapkan  [e] Edit dulu  [t] Tolak  [?] Detail                       │
│                                                                           │
├─ subagent: whisker ● sphinx ● tinker ○ ─── token: 12.4k ── 3 tool ───────┤
│ purr> ▊                                                                   │
└───────────────────────────────────────────────────────────────────────────┘
```

- **Transcript** (tengah, scroll): prompt user, jawaban, card tool call
  (nama tool, argumen ringkas, durasi, hasil ringkas), dan card subagent
  (nama, status, ringkasan terakhir).
- **Preview edit** (card khusus): diff sebelum tulis ke disk; user menyetujui
  dengan `a` / menolak `t` / mengedit `e`. Tidak ada tulis diam-diam.
- **Sidebar/status bar** (bawah): subagent hidup (● jalan, ○ menunggu, ✕ gagal),
  token sesi / batas konteks, jumlah tool call, model aktif, dan preset yang
  aktif (mode + model; bila berbeda dari default, default ditampilkan redup
  ala slim: `tabby · free · default carrier`).
- **Input** (bawah): `purr>` — multi-line (Alt-Enter), history (↑/↓),
  autocomplete slash command (Tab).
- **Warna:** gelap default; satu warna aksen (oranye kucing) untuk aksi yang
  butuh perhatian, hijau untuk sukses, merah untuk gagal/deny. Tanpa warna
  bila terminal tidak mendukung (`NO_COLOR` dihormati). Tema via
  `~/.purrcode/theme.json` (JSON sederhana, bukan sistem tema besar).

### 2.1 Keybinding

| Tombol | Aksi |
|---|---|
| `Ctrl-C` / `Esc` | Interupsi run yang sedang jalan (bukan keluar paksa) |
| `Ctrl-D` (input kosong) | Keluar |
| `↑` / `↓` | History input |
| `Tab` | Autocomplete slash command / path |
| `a` / `t` / `e` (saat preview) | Terapkan / Tolak / Edit dulu |
| `?` | Bantuan keybinding |
| `/` + perintah | Slash command (lihat §2.2) |

### 2.2 Slash command (v0.1)

```
/help /new /resume <id> /fork /compact [fokus] /clear /memory
/export [jsonl|html] /stats /model <provider:model> /agent <nama>
/skill <nama> /plugin list /approve /deny /status /quit
/preset [nama] /model-preset [nama]
```

Semua perintah juga tersedia sebagai subcommand CLI (`purrcode session list`, dst.)
agar bisa dipakai dari skrip.

### 2.3 Mode non-TUI

- `purrcode -p "<prompt>"` — one-shot, output teks biasa ke stdout.
- `purrcode -p "<prompt>" --json` — NDJSON event stream (satu event per baris,
  kompatibel dengan pola pi/omp RPC): `{"type":"tool_call",...}`.
- `purrcode exec < skrip` — headless untuk CI; exit code non-nol bila agen gagal.

## 3. Telegram (v0.2) — desain channel

Satu event bus, render berbeda. Aturan pemetaan TUI → Telegram:

| Event TUI | Telegram |
|---|---|
| Jawaban agen (streaming) | Pesan diedit bertahap (bila channel mendukung `edit`), atau pesan utuh bila tidak |
| Card tool call | Pesan ringkas: `🔧 read src/auth.rs — ok (12 ms)`; detail hanya bila diminta (`/detail`) |
| Preview edit | Pesan diff (monospace, potong bila > 4000 karakter) + tombol inline `[✅ Terapkan] [❌ Tolak]` |
| `ask` picker | Poll/tombol inline dengan opsi — agen lanjut setelah user memilih |
| Approval | Tombol inline `[Ya] [Tidak] [Selalu (sesi ini)]`; timeout 120 dtk → deny + pesan "ditolak otomatis (timeout)" |
| Pesan panjang | Chunking 4096 karakter, potong di batas baris, sambung dengan `(1/3)` |
| Subagent spawn/selesai | Satu baris status: `🐾 Whisker selesai — 6 file ditemukan` |

- **Sesi per chat:** tiap chat Telegram = sesi sendiri; `/new` mulai sesi baru,
  `/resume` menampilkan daftar sesi.
- **Hanya `allow_from`:** daftar chat ID di config; chat di luar daftar mendapat
  balasan sekali: "bot ini privat" lalu diabaikan. Daftar kosong = tidak ada yang
  boleh (fail-closed, bukan warning).
- **Grup vs DM:** default hanya DM (chat 1:1 dengan bot). Grup diizinkan hanya
  bila chat ID grup itu masuk `allow_from` secara eksplisit; bot di grup hanya
  merespons bila di-mention, supaya tidak membanjiri grup. Mode preset berlaku
  per sesi chat; `oyen` via Telegram tetap terikat lapis provider + approval
  (god ≠ yolo).
- **Privasi:** bot tidak menyimpan isi chat di luar `~/.purrcode/`; tidak ada
  analytics/telemetri; kredensial tidak pernah dikirim ke LLM tanpa filter.

## 4. WebUI (v0.4) — desain channel

WebUI khusus PurrCode, ala DeepSeek Harness (`dsh web`): **bukan desktop app**,
bukan pengganti TUI — channel tipis di atas event bus + REST/SSE yang sama.

```
┌─ sidebar ────────┬─ transcript ───────────────────────┬─ inspector ──────┐
│ 🐾 PurrCode      │ sesi: abc123 · ollama:qwen3:8b     │ Tool call        │
│ ──────────────   │                                      │ read src/auth.rs │
│ Sesi             │ > perbaiki bug login…                │ ok · 12 ms       │
│  abc123 ●        │ ◈ Whisker … [running]               │ ──────────────── │
│  def456          │ ◈ Sphinx … [done]                    │ Subagent         │
│ ──────────────   │ ┌─ Preview: src/auth.rs ─────────┐   │ whisker ●       │
│ Plugin           │ │ - if pw.is_empty() …           │   │ sphinx ●         │
│  my-tool ●       │ └────────────────────────────────┘   │ tinker ○          │
│  mcp-git ○       │ [Terapkan] [Tolak]                   │ ──────────────── │
│ ──────────────   │                                      │ Memory           │
│ Scheduler        │ purr> [________________] [Kirim]     │ 128 fakta · MD   │
│  pagi 08:00 ●    │                                      │                  │
└──────────────────┴──────────────────────────────────────┴──────────────────┘
```

- **Halaman:** Sesi (daftar + transcript + tool card + preview edit), Plugin
  (mount/unmount, daftar hook, config), Skill (daftar SKILL.md, tambah/edit),
  Memory (MEMORY.md editor + fakta backend aktif), Scheduler (cron bahasa alami, daftar job).
- **Bukan core kedua:** semua aksi WebUI memanggil API core yang sama dengan TUI;
  tidak ada logika agen di frontend.
- **Keamanan:** listen `127.0.0.1` default; akses jarak jauh hanya via token +
  TLS (atau reverse proxy user sendiri); tanpa akun/cloud; timeout sesi.
- **Teknologi frontend (usulan, bukan keputusan final):** halaman statis +
  SSE, tanpa framework berat — sejalan dengan prinsip ringan. Framework persisnya
  diputuskan saat fase v0.4 dimulai.

## 5. Bahasa — aturan adaptif (keputusan user 2026-11-11)

**Bahasa balasan agen mengikuti user dan model LLM yang dipakai** — seperti AI
coding agent pada umumnya. Bukan bahasa yang dikunci di config.

- **Deteksi per percakapan:** bahasa balasan = bahasa pesan user terakhir
  (Indonesia bila user menulis Indonesia, Inggris bila user menulis Inggris,
  dst.). Bila user ganti bahasa di tengah sesi, agen mengikuti tanpa perlu
  perintah khusus.
- **Bahasa model:** bila model yang dipakai merespons dalam bahasa tertentu
  (mis. model DeepSeek yang cenderung Inggris), agen menyesuaikan agar tidak
  terjadi campuran yang janggal — tapi bahasa user tetap prioritas.
- **Yang tetap Inggris:** nama perintah (`/compact`, `purrcode onboard`),
  nama file, kode, komentar kode, dan config. Dokumen repo (PRD/ARCHITECTURE/
  DESIGN ini) ditulis Indonesia karena user menulis Indonesia — dan akan
  mengikuti bila user beralih bahasa.
- **Label UI TUI/WebUI/Telegram:** mengikuti bahasa sesi yang aktif (default
  mengikuti bahasa user saat pertama kali onboarding); tidak ada menu ganti
  bahasa yang rumit — bahasa adalah konsekuensi dari percakapan, bukan pengaturan.

## 6. Suara & penamaan

- **Nama:** PurrCode AI. Binary: `purrcode`. Prompt: `purr>`. Agen (Set Clowder,
  keputusan user 2026-11-11): Meowstro (orchestrator), Whisker (explorer),
  Sphinx (oracle), Calico (designer), Tinker (fixer), Clowder (council).
  Tim agen disebut **The Clowder**, bukan Pantheon — nama asalnya dari Pantheon
  oh-my-opencode-slim, tapi identitasnya dirombak ke tema kucing.
- **Suara agen:** ringkas, langsung ke hasil, dalam bahasa user saat itu. Tanpa
  korporat filler ("Tentu!", "Pertanyaan bagus!"). Bila tidak tahu, bilang tidak
  tahu + apa yang sudah dicek. Bila hasil tidak terverifikasi, beri label begitu —
  jangan presented as fact (gaya kerja yang user hargai: verify-first).
- **Pesan error:** sebut apa yang gagal, file/perintah yang terlibat, dan satu
  langkah perbaikan konkret. Jangan tampilkan stack trace mentah ke user kecuali
  `--debug`.

## 7. Onboarding & konfigurasi — desain UX

```
$ purrcode onboard
🐾 Selamat datang di PurrCode AI.

  Provider model? [1] Ollama (lokal, gratis)  [2] Anthropic  [3] OpenAI
                [4] Google Gemini  [5] DeepSeek  [6] OpenRouter  [7] Custom endpoint
  > 1
  Model Ollama? (terdeteksi: qwen3:8b, deepseek-coder:6.7b)
  > qwen3:8b
  Preset model? [1] free (lokal + gratis, disarankan)  [2] local (semua Ollama)
                [3] premium (flagship, butuh key berbayar)
  > 1
  Preset mode? [1] tabby (default, disarankan)  [2] carrier (paling aman)
  > 1
  Telegram? (nanti bisa diaktifkan) [y/N]
  > N

  ✓ Config tersimpan di ~/.purrcode/purrcode.jsonc
  ✓ Coba: purrcode -p "jelaskan isi repo ini"
```

- Onboarding tidak pernah meminta API key di terminal dengan echo; token
  dimasukkan via `purrcode auth login <provider>` yang menyimpannya 0600,
  atau via variabel env `PURRCODE_API_KEY` (satu provider aktif).
- `purrcode doctor`: cek provider, model, konteks minimal 64K, direktori,
  izin tulis — dan laporkan apa yang gagal dengan perbaikan yang disarankan.
- Config: JSONC berlapis (system → global → proyek), contoh lengkap dengan
  komentar di `purrcode.jsonc.example`. Kesalahan config = pesan jelas +
  baris yang salah, bukan crash tanpa jejak.

## 8. Gaya repo PurrCode sendiri (cermin gaya coding user)

Dokumen ini juga mengatur bagaimana kode PurrCode ditulis — karena user meminta
agen "sesuai gaya codinganku". **Catatan: tahap saat ini hanya dokumentasi;
aturan di bawah berlaku saat implementasi dimulai.**

1. **Bahasa Rust, lisensi GPL-3.0.** Setiap file sumber membawa header lisensi
   GPL-3.0 singkat; `LICENSE` (GPL-3.0) di root; `cargo deny` di CI memastikan
   semua dependensi kompatibel GPL.
2. **Struktur proper multi-file** (bukan satu file campur aduk): workspace Cargo,
   rantai crate satu arah (ala OpenHuman, ditegakkan CI), tiap crate punya
   tanggung jawab tunggal.
3. **Rust idiomatik:** `cargo fmt` + `cargo clippy -D warnings` di CI; error
   di-wrap dengan konteks (`anyhow`/`thiserror`); tanpa `panic!`/`unwrap()`/
   `expect()` di jalur produksi; tanpa komentar moral/"educational only".
4. **Verifikasi sebelum klaim:** tiap fitur disertai test (unit + skenario
   PRD §4.5); hasil test dicatat di PR, bukan diklaim.
5. **Git workflow:** `main` stabil; fitur di branch; PR dengan deskripsi +
   bukti test; **push hanya atas persetujuan eksplisit user per-aksi**
   (aturan yang sama dengan repo user lainnya).
6. **Dokumen hidup:** PRD/ARCHITECTURE/DESIGN diperbarui bila keputusan berubah;
   perubahan keputusan dicatat di `docs/DECISIONS.md` (format ADR singkat:
   konteks → keputusan → konsekuensi). Revisi 2026-11-11 (Rust, GPL-3.0, bahasa
   adaptif, provider terkenal + custom endpoint, WebUI v0.4, docs-only) adalah
   ADR pertama yang dicatat — lihat §9.
7. **Dependensi:** hanya yang kompatibel GPL; daftar + lisensinya di
   `docs/DEPENDENCIES.md` (dibuat saat implementasi).

## 9. ADR-001 — Keputusan 2026-11-11 (dicatat di DESIGN, dipindah ke DECISIONS.md saat implementasi)

- **Konteks:** Draf v0.1 mengusulkan Go, bahasa balasan Indonesia (terkunci),
  provider generik, tanpa WebUI, dan membuka kemungkinan mulai kode.
- **Keputusan user 2026-11-11 00:24 WIB:** Rust; GPL-3.0; bahasa balasan adaptif
  (mengikuti user & model LLM); provider terkenal dulu + custom endpoint;
  WebUI khusus ala DeepSeek Harness di v0.4; tugas saat ini dokumentasi &
  arsitektur saja, bukan menulis kode.
- **Konsekuensi:** ARCHITECTURE Bab 9 dipetakan ulang ke Rust (ratatui/tokio/
  rusqlite, plugin dua transport); PRD §2/§4/§7 dan DESIGN §4/§5/§8 direvisi;
  repo publik/privat + model default tetap terbuka.

## 10. ADR-002 — Meowstro tidak menulis kode (2026-11-11 00:38 WIB)

- **Konteks:** Meowstro adalah orchestrator + perwakilan user. Kalau ia ikut
  menulis kode, diff/file/log membanjiri konteks utamanya → cepat penuh →
  compaction memotong rencana.
- **Keputusan user:** Meowstro paham codebase tapi tidak pernah menulis/mengubah
  kode; implementasi selalu didelegasikan (ke Tinker dkk). Tool set-nya
  read-only + delegasi; `write`/`edit`/`bash` tidak ada di preset-nya.
- **Konsekuensi:** penegakan di level harness (preset tool set, ditolak runtime),
  bukan sekadar instruksi prompt; PRD §4.5 butir 6 + ARCHITECTURE §3.4
  mencatat kontraknya; konteks utama hanya berisi peta + ringkasan + keputusan.

## 11. ADR-003 — Mode, preset & guardrail berlapis (2026-11-11 00:47 WIB)

- **Konteks:** Satu preset hardcode akan mengunci PurrCode di satu perilaku;
  guardrail perlu beberapa mode (strict/default/god), dan tiap provider punya
  ToS sendiri yang tidak bisa di-bypass harness.
- **Keputusan user:** Preset = file JSONC `preset/v1` (bukan kode); tiga sumbu
  ortogonal (approval × guardrail × sandbox); strict = paling aman (tak ada
  hal ilegal); default = supervised + baseline legal; god = guardrail sisi agen
  mati (nama display: Carrier/Tabby/Oyen — diputuskan user 2026-11-11 00:50 WIB); fallback chain
  tidak dipakai untuk menghindar dari penolakan kebijakan provider.
- **Konsekuensi:** ARCHITECTURE §7 ditulis ulang (preset + 3 sumbu + lapis
  provider); PRD §4.6 + F10 + §7 butir 10–11; nama display ketiga preset diputuskan (Carrier/Tabby/Oyen).

## 12. ADR-004 — Preset model ala slim (2026-11-11 00:52 WIB)

- **Konteks:** User minta preset model seperti oh-my-opencode-slim
  (`docs/presets`: authors, $30, $40, openai, opencode-go, preset-switching —
  dicek live 2026-11-11, daftar file sama dengan clone `10fcee4`).
  Di slim, preset = konfigurasi per agen (model + variant + temperature +
  options + fallback + `extends`) yang diikat ke sesi via `/preset`.
- **Keputusan user:** PurrCode punya preset model terpisah dari preset mode:
  `free` (default), `local`, `premium`, + custom (nama file bebas).
  Berlaku untuk subagent; Meowstro ikut model sesi. Binding per sesi
  (snapshot), tanpa restart. Yang diatur hanya setelan inferensi.
- **Konsekuensi:** ARCHITECTURE §3.5 baru + perbaikan §7.1 (preset mode tidak
  lagi membawa model per subagent); PRD §4.7 + §7 butir 12–13; DESIGN §2.2
  (`/model-preset`) + §7 (wizard menanyakan preset model & mode).
- **Yang TIDAK diadopsi dari slim:** perilaku v1 yang butuh reload setelah
  ganti preset; preset yang mengatur prompt/tools/skill (di PurrCode itu
  urusan file agen, bukan preset model); menyalin model ID preset slim
  mentah-mentah (model ID di file PurrCode adalah contoh yang bisa diedit).

## 13. ADR-005 — Tinjauan ulang dokumentasi (2026-11-11 00:54 WIB)

- **Konteks:** Tinjauan ulang menyeluruh atas PRD/ARCHITECTURE/DESIGN setelah
  preset mode + preset model ditambahkan. Fokus tetap di proyek (bukan repo
  publik/privat — itu sengaja tidak dibahas di ronde ini).
- **Temuan & perbaikan:** penomoran §11 ARCHITECTURE kacau (1–5, 13, 14, 6–12)
  → diurutkan 1–14 + duplikat memory digabung; cross-ref PRD §4.3 salah
  (kontrak subagent ada di §4.5); tree direktori §8 belum memuat
  `presets/`/`model-presets`/snapshot sesi; TUI belum menampilkan preset aktif;
  celah aturan: job cron, kapabilitas plugin, rotasi audit, model mati di
  preset, interaksi dua fallback chain.
- **Keputusan (usul, koreksi bila berbeda):** cron tidak pernah `oyen`/`yolo` +
  allowlist yang dideklarasikan saat job dibuat; plugin default-deny atas
  kapabilitas (secret tidak diteruskan tanpa persetujuan); audit dirotasi +
  dikompresi (retensi ≥30 hari); model preset mati → fallback berlapis +
  peringatan; fallback agen dulu, lalu provider, berhenti total saat penolakan
  kebijakan; `ask` tetap di tool set Meowstro (sebagai perwakilan user ia harus
  bisa bertanya langsung).
- **Konsekuensi:** ARCHITECTURE §3.1/§3.5/§5/§7/§8/§9/§11, PRD §4.3–§4.5/§5/
  §6/§7/F1/F13/F19, DESIGN §1/§2/§3; versi dokumen naik ke 0.3.

## 14. ADR-006 — Skill bawaan (2026-11-11 01:01 WIB)

- **Konteks:** User minta skill bawaan seperti Muse Code (28 skill) dan
  Claude Code (`anthropics/skills`, 19 skill — diverifikasi via pencarian web
  2026-11-11: `document-skills` = docx/pdf/pptx/xlsx, `example-skills` =
  sisanya, termasuk frontend-design, webapp-testing, skill-creator,
  mcp-builder).
- **Keputusan:** Bundel v0.1 = 8 skill saja (`plan`, `review`, `git`, `debug`,
  `scaffold`, `frontend-design`, `webapp-testing`, `doctor`) — ditulis sendiri,
  pola dari kedua inventaris, bukan salinan. Alasannya ekonomi konteks: tiap
  deskripsi skill membebani ~100 token metadata saat startup. Kandidat v0.2:
  `create-skill`, `create-plugin`, `migrate`, + skill dokumen dari nol.
- **Batas lisensi:** 4 skill dokumen Anthropic berlisensi source-available
  yang melarang karya turunan — jangan diturunkan. Skill contoh Anthropic
  Apache-2.0 (pola boleh dipelajari). Skill Muse Code tanpa lisensi jelas —
  hanya pola.
- **Konsekuensi:** PRD F8 + ARCHITECTURE §4 diperluas; nama skill tetap
  fungsional Inggris (kompatibel ekosistem agentskills.io — skill bisa
  dipakai bersama Claude Code), tema kucing tetap di nama agen, bukan di
  nama skill.
