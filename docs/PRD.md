# PurrCode AI — Product Requirements Document (PRD)

Versi: 0.3 (revisi tinjauan ulang) — 2026-11-11
Status: draft untuk persetujuan user. Fokus: **Terminal CLI / TUI dulu**.
Telegram = fase 2. WebUI khusus = fase mendatang (v0.4).
Referensi: `docs/00-analysis-references.md`.

**Keputusan user 2026-11-11 00:24 WIB (mengikat, menggantikan draf v0.1):**
bahasa implementasi **Rust**, lisensi **GPL-3.0**, bahasa balasan agen **adaptif**
(mengikuti bahasa user dan model LLM yang dipakai — seperti AI coding agent pada
umumnya, bukan bahasa yang dikunci), provider = provider terkenal + custom endpoint,
dan nanti ada WebUI khusus ala DeepSeek Harness. **Tugas saat ini hanya dokumentasi
dan arsitektur — bukan menulis kode.**

## 1. Visi

PurrCode AI adalah agen AI modular milik user — bukan sekadar coding agent biasa.
Satu **orchestrator utama** yang menerima prompt user, mendelegasikannya ke **subagent
spesialis yang tidak kehilangan konteks**, mengingat user lintas sesi, dan bisa
dijalankan dari terminal maupun Telegram — ringan, single binary, gratis jalurnya
(BYO model / Ollama lokal), di hardware low-end.

Satu kalimat: **Muse/Hermes yang ringan, modular, dan sesuai gaya coding user.**

## 2. Prinsip produk (tidak bisa ditawar)

1. **Free-first.** Tidak ada langganan wajib, tidak ada jalur berbayar sebagai
   satu-satunya jalan. Model: BYO API key + Ollama/LM Studio lokal sebagai default
   gratis. Managed route berbayar **dilarang** sebagai dependensi.
2. **Rust, single binary, GPL-3.0.** Bukan Python, bukan desktop app. Satu binary
   statis (musl) yang jalan di Linux/macOS/Windows, OpenWrt router/STB 2 GB RAM,
   dan postmarketOS phone. Lisensi proyek: **GNU GPL v3** — semua dependensi harus
   kompatibel GPL; kode referensi GPL (OpenHuman) boleh dipelajari polanya, kode
   non-komersial/proprietary tetap hanya pola, bukan salinan.
3. **Modular seperti DeepSeek Harness, tapi ringan.** Model adapter, tool registry,
   agent loop, session, memory, UI — semuanya plugin yang bisa diganti dari
   konfigurasi. Tanpa mem-vendor framework berat (Cordis) dan tanpa 1,27 jt baris.
4. **Subagent tidak hilang konteks.** Continuation ID + handoff file + registry
   (bukan janji prompt semata). Lihat §4.5.
5. **Supervised fail-closed.** Default: agen bertanya sebelum aksi destruktif,
   delegasi default terbatas, audit log selalu ada. Jangan pernah default `yolo`
   (pelajaran dari oh-my-pi).
6. **Satu engine, banyak surface.** TUI, Telegram, lalu WebUI berbagi core,
   memori, dan skill yang sama (pola opencode + Claude Code).
7. **Bahasa balasan adaptif.** Agen membalas dalam bahasa yang dipakai user di
   percakapan itu (dan selaras dengan model LLM yang dipakai) — seperti AI coding
   agent pada umumnya. Tidak ada bahasa balasan yang dikunci di config; yang
   dikunci hanya bahasa kode/perintah (Inggris). Bila user ganti bahasa di tengah
   sesi, agen mengikuti.
8. **Gaya coding user:** struktur proper/rapi multi-file (bukan satu file campur aduk),
   tiap perubahan terverifikasi terhadap tree nyata, tanpa push tanpa persetujuan
   eksplisit per-aksi.

## 3. Pengguna & konteks

- **Pengguna utama:** latifangren (Kebumen, Indonesia) — builder hands-on, GitHub
  `latifangren`, kerja paralel di PC Ubuntu + HP Android/postmarketOS + server.
  Token/provider: gratis dulu.
- **Perangkat target:** PC Ubuntu (dev), VPS/server Ubuntu (Ollama + daemon),
  OpenWrt router/STB 2 GB RAM 4-core, HP postmarketOS (whyred, 3,54 GiB RAM).
- **Bukan pengguna:** tim enterprise, pengguna yang butuh managed cloud / SSO /
  dashboard admin. Itu non-goal v0.1–v0.4.

## 4. Ruang lingkup

### 4.1 v0.1 — Terminal CLI / TUI (fase ini)

Fungsional:
- F1. **TUI interaktif** (`purrcode`): transcript streaming, tool call sebagai card,
  preview edit sebelum ditulis ke disk, `ask` tool untuk ambiguitas (pilihan
  terstruktur di tengah turn), sidebar status subagent, status bar (model, preset
  mode, preset model, token, sesi). Keyboard-first; mouse opsional.
- F2. **Mode non-TUI sejak hari pertama:** `purrcode -p "<prompt>"` (one-shot),
  `purrcode --json` (event stream NDJSON), `purrcode exec` (headless untuk skrip/CI).
- F3. **Agent loop:** gather context → take action (tool) → verify → jawab.
  Interrupt (Ctrl-C / Esc) membatalkan run yang sedang jalan; steering (pesan susulan
  di tengah run) masuk antrean satu-per-satu.
- F4. **Meowstro + The Clowder (Set Clowder, keputusan user 2026-11-11)** —
  adaptasi Pantheon oh-my-opencode-slim, tapi nama dan identitasnya dirombak
  ke tema kucing PurrCode:

  | Agen | Peran | Model default |
  |---|---|---|
  | Meowstro | Orchestrator & perwakilan user: memahami codebase (peta arsitektur, pola, file kunci) TAPI TIDAK PERNAH menulis/mengubah kode — semua implementasi didelegasikan ke Tinker dkk. Konteksnya dijaga tetap ramping: hanya ringkasan terstruktur dari subagent (lihat §4.5 butir 6) | model utama user |
  | Whisker | Explorer: cari kode/pola, scouting codebase (menyerap Librarian: baca docs/arsip) | model murah/cepat |
  | Sphinx | Oracle: penasihat strategis, review arsitektur, debugging sulit | model kuat |
  | Calico | Designer: kerja UI/TUI, layout, tema | model utama |
  | Tinker | Fixer: implementasi tugas ber-scope jelas, perbaikan kecil | model cepat |
  | Tuxeshadow | Shadow Auditor: pengawas independen Meowstro — memvalidasi plan sebelum eksekusi & aksi berisiko (advisory default, override tercatat). Tool read-only, tanpa delegasi task. | model cepat/murah |
  | Clowder | Council: panel 2–3 seat (model berbeda) untuk keputusan berisiko/ambigu | fallback chain per seat |

  Agen didefinisikan sebagai **file Markdown + frontmatter**
  (`.purrcode/agents/<nama>.md`: `description`, `tools`, `model`, `permissionMode`)
  ala Claude Code — bukan konstanta di kode (pelajaran dari slim yang menyimpan
  prompt sebagai konstanta TS). User bisa menambah agen sendiri dengan menambah file.
- F5. **Subagent dengan konteks utuh (§4.5).**
- F6. **Tools inti v0.1 (12):** `read`, `write`, `edit` (hash-anchored, preview dulu),
  `bash`, `grep`, `glob`, `find`, `todo`, `ask`, `task` (spawn subagent), `webfetch`,
  `websearch`. Tanpa LSP/DAP di v0.1 (pelajaran oh-my-pi: jangan kejar 32 tool di awal).
- F7. **Memory default MD minimal:** `MEMORY.md`/`SOUL.md`/`USER.md` (+ session
  JSONL) — seperti Hermes & Muse App. Recall/FTS5/eksternal sebagai backend
  plugin di belakang trait `MemoryBackend` (lihat ARCHITECTURE §4); default
  tidak butuh database. Perintah `/compact` (dengan instruksi fokus), `/clear`, `/memory`.
- F8. **Skill bawaan (8, v0.1):** format `SKILL.md` + frontmatter, dimuat
  on-demand (hanya deskripsi di awal; isi penuh saat dipanggil — standar
  agentskills.io, ala Claude Code). Discovery: `.purrcode/skills/` +
  `~/.purrcode/skills/` + bawaan. Bawaan v0.1 (ditulis sendiri, pola dari
  Muse Code 28 skill + `anthropics/skills` 19 skill — bukan salinan):
  `plan` (perencanaan eksplisit), `review` (review kode + hasil kerja),
  `git` (aturan git safety: tanpa commit/push tanpa persetujuan eksplisit),
  `debug` (debugging sistematis: reproduksi → hipotesis → perbaiki → regresi),
  `scaffold` (scaffolding proyek greenfield), `frontend-design` (desain UI
  anti "AI slop"), `webapp-testing` (uji web app via browser), `doctor`
  (diagnosis instalasi/runtime PurrCode). Skill ditulis manusia di v0.1;
  kurator otomatis ditunda v0.3. Kandidat v0.2: `create-skill`,
  `create-plugin`, `migrate` (impor memory/MCP dari Claude Code/Codex/Muse),
  dan skill dokumen (`pdf`, `docx`, `xlsx`, `pptx`) — ditulis dari nol,
  BUKAN turunan dari 4 skill dokumen Anthropic yang lisensinya
  source-available dan melarang karya turunan. Tiap deskripsi skill adalah
  kontrak routing (apa yang memuat / tidak memuat skill itu).
- F9. **Plugin:** manifest `plugin.yaml` + hook (`on_start`, `on_session_start`,
  `pre_tool_use`, `post_tool_use`, `on_stop`), discovery `~/.purrcode/plugins/` +
  `.purrcode/plugins/`. Plugin bawaan dikompilasi sebagai feature Cargo;
  plugin eksternal lewat proses terpisah + protokol JSON-RPC stdio (pola subagent
  DeepSeek Harness yang memakai runtime ter-pin, bukan PATH). MCP sebagai tool eksternal.
- F10. **Approval & keamanan:** mode `supervised` default (tanya sebelum write/edit/
  exec di luar workspace; deny > ask > allow; audit log JSONL). `workspace-write`
  default; `danger-full-access` hanya via flag eksplisit + konfirmasi.
  Workspace confinement default (`restrict_to_workspace: true`). Mode & preset
  lengkap: §4.6 (preset = file, 3 sumbu, strict/default/god, lapis ToS provider).
- F11. **Session:** resume/fork/branch (`/resume`, `/fork`), compaction non-destruktif,
  ekspor JSONL/HTML (`/export`). Konteks proyek dari `AGENTS.md` / `CLAUDE.md` /
  `.purrcode/SYSTEM.md` (hierarkis).
- F12. **Provider — yang terkenal dulu + custom endpoint.** Provider bawaan v0.1:
  Anthropic (Claude), OpenAI, Google Gemini, DeepSeek, xAI, Mistral, Groq, Cerebras,
  OpenRouter, Kimi, MiniMax — ditambah **custom endpoint OpenAI-compatible**
  (URL + key sendiri) dan lokal **Ollama / LM Studio** (`ollama:<model>`).
  Grammar `provider:model`, fallback chain model dengan cooldown. Syarat konteks
  minimal ditegakkan di startup (model < 64K konteks ditolak dengan pesan jelas,
  ala Hermes) — karena loop multi-step butuh headroom. Menambah provider baru =
  menambah adapter + satu baris di registry, bukan mengubah core. Model per agen
  diatur preset model (§4.7), bukan hardcode.
- F13. **Config berlapis:** system → global `~/.purrcode/purrcode.jsonc` → proyek
  `.purrcode/purrcode.jsonc`, merge ramah-gagal. Onboarding `purrcode onboard`
  (wizard: provider, model, preset model, preset mode).
- F14. **Build & distribusi:** satu binary statis musl, `cargo` feature-gating
  (ala ZeroClaw: tanpa feature `agent-runtime` yang ada hanya kernel),
  cross-compile amd64/arm64/arm/mipsle. `purrcode service install` (systemd +
  OpenRC, ala ZeroClaw) — daemon mode untuk fase 2.

Non-goal v0.1: Telegram, WebUI, cron/scheduler, LSP/DAP, browser automation, collab/
live-share, desktop app, sandbox OS-level (Landlock/seccomp — roadmap v0.3),
plugin WASM, multi-user/fleet.

### 4.2 v0.2 — Telegram (seperti Hermes Agent)

- F15. **Gateway channel:** `purrcode gateway` — satu daemon, satu memori, banyak
  channel; Telegram pertama. Long-poll `getUpdates` (hand-rolled di atas reqwest,
  tanpa SDK berat — ala ZeroClaw) atau teloxide; chunking 4096 karakter;
  retry-after 429.
- F16. **Abstraksi channel:** interface inti + capability opt-in (typing/edit/
  reaksi/streaming) ala PicoClaw. TUI dan Telegram me-render event yang sama
  dari event bus core.
- F17. **Approval via Telegram:** tombol inline Yes/No/Always (sesi ini) untuk
  aksi yang butuh persetujuan; timeout 120 dtk → deny (fail-closed).
- F18. **Sesi per chat:** tiap chat Telegram = sesi sendiri; `/new`, `/resume`
  via perintah bot. Hanya `allow_from` (daftar chat ID) yang boleh bicara dengan
  bot — **fail-closed** (kosong = tidak ada yang boleh, bukan warning).

### 4.3 v0.3 — Otonom penuh

- F19. Cron bahasa alami ("tiap pagi jam 8 ringkas email…") + delivery queue +
  allowlist tool fail-closed (ala Hermes). Job cron tidak pernah `oyen`/`yolo`;
  aksi tulis harus masuk allowlist yang dideklarasikan saat job dibuat — di luar
  itu job gagal + memberi tahu user (lihat ARCHITECTURE §7).
- F20. Skill self-improving: kurator yang mengusulkan skill dari tugas kompleks
  (5+ tool call), user menyetujui sebelum skill disimpan (bukan otomatis diam-diam).
- F21. Sandbox OS-level (Linux Landlock/seccomp; macOS Seatbelt) + mode
  `read-only` / `workspace-write` / `danger-full-access`.
- F22. Heartbeat/proactive: cek berkala, nudge bila ada yang berubah dan layak
  disampaikan (satu baris per thread yang menunggu — jangan kejar-kejaran).

### 4.4 v0.4 — WebUI khusus (ala DeepSeek Harness)

- F23. **`purrcode web`:** WebUI lokal khusus PurrCode (bukan desktop app) —
  daftar sesi, transcript + tool card yang sama dengan TUI, manajemen plugin
  (mount/unmount dari UI), manajemen sesi/skill/memory, dan penjadwalan tugas.
  Satu engine yang sama dengan TUI/Telegram (core + event bus + REST/SSE);
  WebUI adalah channel tipis seperti yang lain, bukan core kedua.
- F24. WebUI tidak menggantikan TUI: TUI tetap surface utama untuk coding;
  WebUI untuk pemantauan jarak jauh, manajemen plugin/sesi, dan pemakaian dari HP.
- F25. Keamanan WebUI: hanya listen di localhost secara default; akses jarak jauh
  hanya via token + TLS (atau reverse proxy user sendiri); tanpa akun/cloud.

### 4.5 Subagent tidak hilang konteks — kontrak (inti PRD ini)

Masalah yang diselesaikan: di oh-my-opencode-slim, anti-hilang-konteks hanya disiplin
prompt ("amend the same child, never respawn") — rapuh. PurrCode menambahkan lapis teknis:

1. **Continuation ID.** Setiap delegasi mendapat `purr_<id>` yang stabil. Melanjutkan
   pekerjaan = memanggil ID yang sama. **Respawn dilarang** oleh orchestrator
   (ditegakkan di kode, bukan di prompt): `task()` dengan ID yang sudah ada =
   lanjutkan child itu.
2. **Handoff file.** Setiap delegasi menulis dua file di session dir:
   `subagents/<id>/brief.md` (tugas, path/line, format output yang diharapkan,
   batasan) dan `subagents/<id>/result.md` (hasil terstruktur). Child bisa dibaca
   ulang kapan saja; parent tidak bergantung pada ingatan model.
3. **Registry.** Meowstro menyimpan `subagents/<id>.json` (status, parent,
   ringkasan terakhir, token dipakai). TUI menampilkan daftar subagent hidup.
4. **Ringkasan kembali, bukan konteks mentah.** Child mengembalikan ringkasan
   terstruktur (apa diubah, file:line, apa yang gagal, apa yang belum diverifikasi)
   — parent tidak menelan seluruh transcript child (pelajaran Claude Code).
   Bila parent butuh detail, ia membaca `result.md` / melanjutkan child via ID.
5. **Batas:** tanpa nesting subagent di v0.1 (child tidak spawn child); kedalaman
   maks 1. Background mode didukung; follow-up via ID.
6. **Meowstro tidak menulis kode (aturan harness, keputusan user 2026-11-11
   00:38 WIB).** Meowstro adalah petinggi/perwakilan user, bukan tukang:
   tugasnya memahami codebase, menyusun rencana, mendelegasikan, dan menolak
   atau menerima hasil — bukan mengimplementasikan. Alasannya konteks: konteks
   Meowstro adalah konteks utama yang dibawa antar turn; kalau ia ikut menulis
   kode, diff + isi file + log error membanjirinya, konteks cepat penuh, lalu
   compaction memotong rencana. Jadi detail tinggal di subagent (di
   `brief.md`/`result.md` mereka), Meowstro hanya memegang peta + ringkasan +
   keputusan. Yang ditegakkan:
   - **Tool set Meowstro read-only + delegasi:** `read`, `grep`, `glob`,
     `find`, `todo`, `ask`, `task`. `write`, `edit`, dan `bash` **tidak ada**
     di preset-nya — penegakan di level harness (preset didaftarkan sebagai
     tool set agen; runtime menolak pemanggilan tool di luar preset), bukan
     sekadar instruksi di system prompt, karena instruksi prompt bisa dilanggar
     model.
   - **Paham codebase ≠ hafal tiap baris.** Peta codebase dibuat Whisker
     (scouting + ringkasan arsitektur); Meowstro boleh membaca file kunci
     sendiri secukupnya (`read` terbatas) saat butuh memahami keputusan —
     tapi tidak pernah menulis.
   - **Verifikasi tanpa menulis.** Meowstro memverifikasi hasil lewat laporan
     terstruktur Tinker + membaca diff/file hasil secukupnya, atau meminta
     Whisker mengaudit hasil Tinker (independent oracle) — bukan dengan
     memperbaiki sendiri.
   - **Pengecualian: tidak ada.** Bila ada tugas yang "lebih cepat dikerjakan
     sendiri", Meowstro tetap mendelegasikannya ke Tinker; kecepatan sesaat
     tidak boleh mengorbankan konteks utama.
   - **`ask` tetap ada** (default; koreksi bila berbeda): sebagai perwakilan
     user, Meowstro harus bisa bertanya langsung saat keputusan butuh user —
     itu bagian dari tugasnya sebagai petinggi, bukan pekerjaan implementasi.
7. **Tuxeshadow: The Shadow Auditor (pengawas independen Meowstro, keputusan user 2026-11-11).**
   Meowstro adalah *single point of failure*: jika rencananya salah, eksekusi Tinker akan keliru.
   Tuxeshadow bertindak sebagai critic/auditor independen yang mengevaluasi rencana dan aksi berisiko:
   - **Trigger sempit (bukan tiap langkah):** Sebelum rencana disetujui, sebelum aksi destruktif (tulis di luar workspace, hapus, perintah bahaya), saat recovery plan setelah error, dan wajib pada cron job otomatis.
   - **Berdasarkan artefak mentah:** Shadow menilai plan JSON, brief, dan diff asli — bukan ringkasan kata-kata Meowstro (menghindari framing bias).
   - **Non-binding & Single Command Chain:** Shadow tidak memiliki tool `task` (tidak bisa mendelegasikan sendiri). Meowstro dapat menerima catatan Shadow atau meng-override dengan alasan tercatat di audit log (`⚠ [shadow overridden]`).
   - **Kekuatan per mode:** Carrier = keberatan Shadow memblokir (butuh approval user); Tabby = advisory (override tercatat); Oyen = silent auditor (mencatat log saja).

Kriteria penerimaan §4.5: delegasi 3 langkah berantai (Whisker → Sphinx → Tinker)
dengan interupsi di tengah tetap bisa dilanjutkan dari ID yang sama tanpa
mengulang pencarian dari nol — diuji dengan skenario di `ARCHITECTURE.md` §8.

### 4.6 Mode, preset & guardrail — tidak hardcode (keputusan user 2026-11-11)

Mode bukan satu label yang dikunci di kode. Semua mode adalah **preset**:
file JSONC (`preset/v1`) di `presets/` bawaan → `~/.purrcode/presets/` →
`.purrcode/presets/`; dipilih via `--preset` / config / default `tabby`.
Satu preset = kombinasi tiga sumbu: **approval** (manual/supervised/auto/yolo)
× **guardrail** (strict/default/god) × **sandbox** (confinement/os).
Yolo soal approval saja; god mode soal guardrail saja — dua keputusan terpisah.

- **Strict (`carrier`):** paling aman — tidak boleh ada hal berbau ilegal atau
  melanggar hukum; tanpa akses kredensial; egress jaringan terbatas; approval
  manual.
- **Default (`tabby`):** supervised + baseline legal; pekerjaan security user
  atas sistem & kode sendiri tetap diizinkan.
- **God (`oyen`):** guardrail
  sisi agen dimatikan; approval tetap jalan; audit tetap nyala; opt-in eksplisit
  per sesi (`--preset oyen` + konfirmasi, tidak ditawarkan di onboarding).
- **Batasan provider (lapis ToS, tidak bisa di-bypass harness):** tiap provider
  (Claude, OpenAI, Google, Mistral, Kimi, dsb.) menegakkan ToS-nya sendiri.
  Penolakan provider ditampilkan apa adanya; fallback chain tidak dipakai
  untuk menghindar dari penolakan kebijakan — hanya untuk error teknis.
  Model lokal (Ollama / LM Studio) tidak punya lapis ini.
- **Subagent** hanya boleh mempersempit policy parent (tidak bisa naik ke
  mode yang lebih longgar dari parent-nya).

Kriteria penerimaan: preset baru bisa ditambahkan hanya dengan menambah file
(tanpa ubah kode); `purrcode preset list` menampilkan ketiganya; mode `god`
menolak aktif tanpa konfirmasi eksplisit per sesi.

### 4.7 Preset model — mix-model per agen (ala slim)

Terpisah dari preset mode (§4.6): preset model hanya mengatur model inferensi
tiap subagent. Satu file JSONC per preset (`model-presets/*.jsonc`, skema
`model-preset/v1`); **nama preset = nama file; preset custom = file buatan user
dengan nama bebas**. Per agen: `model` (provider:model atau fallback chain),
`variant` (thinking strength), `temperature`, `options` (JSON provider),
`fallback`. Mendukung `extends` satu parent.

- Bawaan v0.1: `free` (default — Ollama lokal + tier gratis; tanpa jalur
  berbayar), `local` (semua Ollama, offline penuh), `premium` (flagship per
  peran, BYO key, dipilih eksplisit — bukan default).
- Berlaku untuk subagent (Whisker/Sphinx/Calico/Tinker/kursi Clowder); model
  Meowstro = model sesi. Urutan menang: model eksplisit di `task()` >
  preset model > frontmatter agen > model sesi.
- Binding per sesi (snapshot di `sessions/<id>/model-preset.json`); ubah
  default global tidak mengubah sesi yang sedang jalan; `/model-preset <nama>`
  mengubah delegasi berikutnya di sesi itu; tanpa restart.
- Perintah: `/model-preset` (manager TUI 3 tingkat: daftar → susunan agen →
  edit agen) + `/model-preset <nama>` (fast path); CLI
  `purrcode model-preset list/show`; flag `--model-preset`.

Kriteria penerimaan: preset `free` bisa dipakai tanpa satu pun key berbayar;
preset custom baru dikenali hanya dengan menambah file; ganti preset di tengah
sesi tidak mengulang pekerjaan child yang sedang berjalan.

## 5. Persyaratan non-fungsional

- **Ringan:** binary ≤ 25 MB (target), RAM idle ≤ 50 MB,
  startup TUI < 1 dtk di PC, < 3 dtk di STB 2 GB. Tanpa runtime berat (tanpa Node/Bun/
  Python di distribusi). Resep build ala ZeroClaw: `opt-level="z"`, LTO, strip,
  musl statis, feature-gating agresif, rustls (bukan OpenSSL).
- **Offline-first sebisa mungkin:** kerja lokal + Ollama tanpa internet; hanya
  panggilan model + `webfetch`/`websearch` yang butuh jaringan.
- **Observabilitas:** semua tool call + approval + subagent spawn tercatat di audit
  log JSONL; `/stats` menampilkan token, biaya (bila ada), durasi.
- **Keterbukaan:** semua state di file yang bisa dibaca manusia (JSONL, Markdown;
  SQLite hanya bila backend sqlite-fts aktif dan bisa di-`sqlite3`). Tidak ada
  state biner misterius.
- **Modular, tanpa breaking change (paling utama):** setiap subsistem (memory,
  provider, channel, plugin) duduk di belakang trait berversi; mengganti
  implementasi tidak mengubah format data atau kontrak yang sudah rilis.
  Format MD memory dan JSONL sesi adalah kontrak stabil.
- **Lisensi:** GPL-3.0. Dependensi harus kompatibel GPL (MIT/Apache-2.0/BSD
  diizinkan; proprietary tidak boleh di-link). Daftar dependensi + lisensinya
  dicatat di `docs/DEPENDENCIES.md` saat implementasi.

## 6. Risiko

| Risiko | Mitigasi |
|---|---|
| Scope membengkak meniru omp (316 MB, 32 tool, LSP/DAP) | v0.1 dikunci 12 tool; fitur baru masuk hanya via PRD revisi |
| Anti-hilang-konteks ternyata tetap rapuh | Uji skenario §4.5; handoff file sebagai jaring pengaman |
| Provider/model berubah-ubah (harga, konteks) | Grammar `provider:model` + fallback chain + custom endpoint; tanpa kunci ke satu vendor |
| Lisensi referensi (GPL/Sustainable Use/proprietary) | Pola saja, bukan kode; daftar larangan di `00-analysis` §1; PurrCode sendiri GPL-3.0 |
| DeepSeek Harness API breaking | Tidak vendor Cordis; pola diimplementasi sendiri |
| Rust: compile lambat, plugin in-process sulit | Plugin eksternal via JSON-RPC stdio; feature-gating agar kernel tetap kecil |
| Model di preset mati/diganti provider | Validasi `purrcode doctor` + fallback berlapis (preset → frontmatter → model sesi) + peringatan, tanpa crash |
| ToS provider berubah (mempengaruhi mode `god`) | Penolakan provider selalu disajikan apa adanya; `purrcode doctor` memeriksa ulang; tidak ada silent fallback |
| Plugin berbahaya (supply chain) | Kapabilitas deklarasi + default deny; secret tidak diteruskan tanpa persetujuan; runtime ter-pin |

## 7. Keputusan (status 2026-11-11 — lihat kolom Status per butir)

| # | Keputusan | Status |
|---|---|---|
| 1 | Bahasa implementasi: **Rust** | ✅ Diputuskan user |
| 2 | Lisensi: **GPL-3.0** | ✅ Diputuskan user |
| 3 | Bahasa balasan: **adaptif** — mengikuti bahasa user & model LLM yang dipakai | ✅ Diputuskan user |
| 4 | Provider: terkenal dulu (Anthropic, OpenAI, Gemini, DeepSeek, xAI, Mistral, Groq, Cerebras, OpenRouter, Kimi, MiniMax) + custom endpoint + Ollama/LM Studio | ✅ Diputuskan user |
| 5 | WebUI khusus ala DeepSeek Harness di fase mendatang (v0.4) | ✅ Diputuskan user |
| 6 | Tugas saat ini: **dokumentasi & arsitektur saja, bukan menulis kode** | ✅ Diputuskan user |
| 7 | Repo publik atau privat? | ✅ **Publik** (`latifangren/purrcode-ai`, keputusan user 2026-11-11) |
| 8 | Model default v0.1 | ✅ **Tidak ada default hardcode** — wajib di-set user lebih dulu via config/onboarding (bisa custom endpoint/proxy seperti Magpie/OmniRoute ala opencode & omp). Menolak jalan tanpa konfigurasi model. (keputusan user 2026-11-11) |
| 9 | Nama binary / perintah: **`purrcode-ai`** | ✅ Diputuskan user (2026-11-11) |
| 10 | Sistem preset & guardrail berlapis (preset = file `preset/v1`, 3 sumbu approval × guardrail × sandbox, strict/default/god, lapis ToS provider, fallback tidak retry atas penolakan kebijakan) | ✅ Diputuskan user (2026-11-11 00:47 WIB) |
| 11 | Nama display ketiga preset: Carrier (strict), Tabby (default), Oyen (god) | ✅ Diputuskan user (2026-11-11 00:50 WIB) |
| 12 | Sistem preset model (free default, local, premium, custom; binding per sesi ala slim v2) | ✅ Diputuskan user (2026-11-11 00:52 WIB) |
| 13 | Model ID contoh di preset `free`/`premium` (perlu verifikasi ketersediaan saat setup) | ⏳ Terbuka — contoh di file, bukan janji |
| 14 | Skill bawaan v0.1 (8): `plan`, `review`, `git`, `debug`, `scaffold`, `frontend-design`, `webapp-testing`, `doctor`; kandidat v0.2: `create-skill`, `create-plugin`, `migrate`, skill dokumen dari nol | ✅ Diputuskan user (2026-11-11 01:03 WIB) |
| 15 | Agen ke-7 The Clowder: **Tuxeshadow** (The Shadow Auditor) — pengawas Meowstro independen, trigger sempit, non-binding default, model hemat | ✅ Diputuskan user (2026-11-11) |
