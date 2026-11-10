# PurrCode AI — Architecture (ARCHITECTURE.md)

Versi: 0.3 (revisi tinjauan ulang) — 2026-11-11
Bahasa implementasi: **Rust** (keputusan user 2026-11-11 00:24 WIB).
Lisensi: **GPL-3.0**. Tugas saat ini: dokumentasi & arsitektur saja — bukan kode.
Arsitektur ini tidak bergantung pada detail bahasa: Bab 2–8 adalah desain;
Bab 9 adalah pemetaan ke Rust.

## 1. Gambaran sistem

```
                    ┌─────────────────────────────────────────────┐
                    │              purrcode (1 binary)            │
                    │              Rust · musl statis             │
                    │                                             │
  user ──► TUI ──┐  │  ┌──────────────┐    ┌──────────────────┐   │
  (v0.2) Telegram ─┤─►│   channels   │───►│  core daemon     │   │
  (v0.4) WebUI ───┤  │  │ (plugin)     │    │  event bus       │   │
  -p / --json ────┘  │  └──────────────┘    │  agent loop      │   │
                    │                      │  tool registry   │   │
                    │                      │  provider adap.  │   │
                    │                      │  session store   │   │
                    │                      │  memory service  │   │
                    │                      │  plugin manager  │   │
                    │                      │  approval gate   │   │
                    │                      └──────┬───────────┘   │
                    │                             │ hooks          │
                    │                      ┌──────▼───────────┐   │
                    │                      │  plugins/skills  │   │
                    │                      └──────────────────┘   │
                    └─────────────────────────────────────────────┘
                              │ files (SQLite/JSONL/MD)
                    ~/.purrcode/  +  .purrcode/ (proyek)
```

Prinsip: **satu engine, banyak surface** (opencode + Claude Code). Channel (TUI,
Telegram, WebUI, `--json`) adalah client tipis di atas event bus — tidak punya logika agen.
Semua keputusan agen terjadi di core; channel hanya me-render event dan mengirim input.

Alur satu turn (agent loop, ala codex `run_turn` + opencode `loop.ts` murni):

```
input user → build prompt (system + MEMORY.md + AGENTS.md + ringkasan sesi)
  → provider.stream() → tool call? ──ya──► approval gate ──► eksekusi tool
  │                                              │                  │
  │                                              ▼                  ▼
  │                                         audit log          event bus → channel
  │                                              │                  │
  └─────────────── tidak ──► jawab + simpan turn (JSONL + SQLite sesi; ingat ke backend memory)
```

Interrupt: `CancellationToken` per run (ala codex tokio). Steering: pesan susulan
masuk antrean, diproses satu-per-satu setelah turn selesai atau sebagai interupsi
bila user menekan Esc.

## 2. Komponen core

| Komponen | Tanggung jawab | Pola asal |
|---|---|---|
| Event bus | Satu aliran event bertipe (turn, tool_call, approval, subagent, error) ke semua channel; buffer + replay untuk `--json` dan WebUI (SSE) | opencode SSE + bus; pi `EventStream` |
| Agent loop | `loop()` murni: pendingCalls → stepMessages → nextRequest; max-steps dipaksa jawab teks | opencode `loop.ts`; codex `run_turn` |
| Tool registry | Snapshot definisi tool (JSON-schema, freeze setelah registrasi); tier approval per tool `read\|write\|exec` | opencode `tool.ts`; oh-my-pi tier |
| Provider adapter | Grammar `provider:model`; satu chokepoint factory; fallback chain + cooldown/rate-limiter; gate konteks minimal 64K | openhuman grammar + gate; picoclaw fallback |
| Session store | JSONL append-only berversi + SQLite (`session`, `turn`, `tool_call`); resume/fork/branch via id/parentId; compaction non-destruktif | pi session tree v3; codex rollout JSONL; opencode SQLite+Drizzle |
| Memory service | Trait `MemoryBackend` (§4); default `MarkdownMemory`, plugin sqlite-fts/eksternal | hermes/Muse MD; picoclaw FTS5 sebagai plugin |
| Plugin manager | Discovery, manifest, lifecycle, hook fan-out, mount/unmount | §5 |
| Approval gate | Rule engine deny > ask > allow; mode supervised default; audit log | §7 |
| Preset & guardrail | Preset file JSONC `preset/v1`; 3 sumbu (approval × guardrail × sandbox); lapis ToS provider | §7 |
| Meowstro (orchestrator) | Work graph, delegasi, registry subagent | §3, PRD §4.5 |
| Sandbox adapter | Interface; v0.1 = workspace confinement + allowlist; v0.3 = Landlock/seccomp/Seatbelt | codex `SandboxMode` (target akhir) |
| Web server (v0.4) | REST + SSE di atas event bus yang sama; auth token; localhost default | opencode server; deepseek-harness web |

## 3. Meowstro & subagent

### 3.1 Agen sebagai file, bukan kode

```
.purrcode/agents/meowstro.md   # orchestrator — frontmatter: description, tools, model, permissionMode
.purrcode/agents/whisker.md    # explorer
.purrcode/agents/sphinx.md     # oracle
.purrcode/agents/calico.md     # designer
.purrcode/agents/tinker.md     # fixer
.purrcode/agents/clowder.md    # council
~/.purrcode/agents/*.md        # agen global user
```

Tim agen disebut **The Clowder** (Set Clowder, keputusan user 2026-11-11) —
bukan "Pantheon": nama-nama di atas adalah adaptasi Pantheon oh-my-opencode-slim
yang dirombak ke tema kucing PurrCode.

Frontmatter (ala Claude Code) + body Markdown sebagai system prompt agen.
Menambah agen = menambah file. Model per agen: urutan penentuan di §3.5
(preset model > frontmatter `model:` > model sesi) — frontmatter hanyalah
default bila tidak ada preset model.

### 3.2 Delegasi — kontrak anti-hilang-konteks

```
Meowstro ──task(id?, brief)──► Subagent[<id>]
                                        │ writes
                                        ▼
                              session/subagents/<id>/brief.md
                              session/subagents/<id>/result.md
                              session/subagents/<id>.json   # registry
                                        │ returns
                                        ▼
                              ringkasan terstruktur ──► Meowstro
```

Aturan yang ditegakkan di kode (bukan di prompt):
1. `task()` tanpa ID → buat ID baru `purr_<rand6>`, simpan registry.
2. `task()` dengan ID yang ada → **lanjutkan child itu** (resume session child,
   brief baru di-append ke `brief.md`). Respawn dilarang: tidak ada path kode
   yang membuat child baru dengan tugas sama.
3. Child mengembalikan ringkasan terstruktur (diubah: file:line; gagal: apa;
   belum terverifikasi: apa). Parent membaca `result.md` bila butuh detail.
4. Tanpa nesting di v0.1: child tidak boleh memanggil `task()`. Batas kedalaman = 1.
5. Background mode: child jalan async (tokio task); follow-up via ID; TUI menampilkan
   status child (running/done/failed) di sidebar.
6. Council (Clowder): 2–3 seat paralel, masing-masing model berbeda (fallback chain);
   Meowstro menggabungkan vonisnya; seat yang gagal tidak menggagalkan council.

### 3.3 Work graph

Meowstro menyusun DAG tugas (Whisker → Sphinx → Tinker, atau paralel bila
independen), mengeksekusi sesuai dependensi, dan menyimpan graph di
`session/workgraph.json` agar bisa dilanjutkan setelah restart.

### 3.4 Meowstro: petinggi, bukan tukang (aturan harness)

Meowstro adalah perwakilan user di dalam sistem dan pemilik konteks utama.
Ia memahami codebase — peta arsitektur, pola, file kunci — tapi **tidak pernah
menulis atau mengubah kode** (keputusan user 2026-11-11 00:38 WIB; detail
kontrak di PRD §4.5 butir 6). Alasannya satu: konteks Meowstro adalah konteks
yang dibawa antar turn dan dipakai untuk semua keputusan; diff + isi file +
log error dari pekerjaan implementasi akan membanjirinya, memaksanya
compaction lebih cepat, dan rencana pun terpotong. Detail pekerjaan tinggal
di subagent (`brief.md`/`result.md`); Meowstro memegang peta + ringkasan +
keputusan.

| Agen | Tool set (preset, ditegakkan runtime) |
|---|---|
| Meowstro | `read`, `grep`, `glob`, `find`, `todo`, `ask`, `task` — **tanpa `write`, `edit`, `bash`** |
| Whisker | `read`, `grep`, `glob`, `find`, `webfetch`, `websearch` (read-only penuh) |
| Sphinx | `read`, `grep`, `glob`, `find`, `ask` |
| Calico | `read`, `write`, `edit`, `grep`, `glob`, `find` |
| Tinker | semua 12 tool v0.1 (satu-satunya agen implementasi penuh) |
| Clowder | `read`, `grep`, `glob`, `find` (seat baca + menilai) |

Penegakan: preset tool set didaftarkan per agen (ala preset Muse Code —
mode = tool set); runtime menolak pemanggilan tool di luar preset dengan
error `tool_not_in_preset`, dan penolakan itu masuk audit log. Instruksi
"jangan menulis kode" di system prompt Meowstro tetap ada sebagai lapis
kedua, tapi bukan penegakan — penegakannya adalah preset. Bila Meowstro
butuh sesuatu yang hanya bisa dilakukan tool yang tidak ia miliki (misalkan
menjalankan test), ia mendelegasikannya ke Tinker dan membaca laporannya.

### 3.5 Preset model — mix-model sebagai file (ala slim)

Dua jenis preset dengan namespace berbeda — jangan dicampur:

- **Preset mode** (`presets/*.jsonc`, §7): perilaku — approval × guardrail ×
  sandbox. Dipilih via `--preset`.
- **Preset model** (`model-presets/*.jsonc`, bagian ini): model inferensi per
  agen. Dipilih via `--model-preset` / `/model-preset`.

Satu model preset = satu file JSONC berskema `model-preset/v1`:

```jsonc
{
  "schema": "model-preset/v1",
  "name": "free",
  "extends": "base",              // opsional, satu parent saja (ala slim)
  "agents": {
    "whisker": { "model": "ollama:qwen2.5-coder:32b" },
    "sphinx":  { "model": ["openrouter:meta-llama/llama-3.3-70b-instruct:free",
                            "ollama:qwen2.5-coder:32b"],
                 "variant": "high" },
    "tinker":  { "model": "ollama:qwen2.5-coder:32b", "temperature": 0.2 }
  }
}
```

Yang diatur HANYA setelan inferensi: `model` (provider:model atau fallback
chain), `variant` (thinking strength: `thinking`/`high`/`low`), `temperature`
(0–2), `options` (JSON khusus provider), `fallback`. Bukan prompt, tools,
permission, atau skill — batas yang sama dengan slim v2: pemilihan preset
sesi tidak mengubah agent registry.

Discovery: bawaan `model-presets/*.jsonc` → `~/.purrcode/model-presets/*.jsonc`
→ `.purrcode/model-presets/*.jsonc`. **Nama preset = nama file; preset custom =
file apa pun yang user buat di sana, nama bebas sesuai masing-masing.**
`extends` satu parent tanpa siklus; yang disimpan hanya override lokal, bukan
hasil flatten — edit base ikut berlaku untuk binding baru atau reapply
eksplisit.

Urutan penentuan model subagent (menang yang paling atas):

1. Model eksplisit di `task()` / lanjutan child (mengambil alih; menonaktifkan
   fallback chain child itu — ala slim).
2. Entri model preset sesi itu untuk agen tersebut.
3. `model:` di frontmatter `.purrcode/agents/<nama>.md`.
4. Model sesi.

Model Meowstro (main agent) TIDAK diatur preset model — ia memakai model sesi
(dari `model` di preset mode / `/model` / config). Preset model hanya mengatur
subagent (Whisker/Sphinx/Calico/Tinker/kursi Clowder) — sama seperti slim yang
preset-nya hanya mengatur inferensi subagent, sementara main agent memakai
pemilihan model native host.

Binding per sesi (diadopsi dari slim v2):

- Saat sesi root mulai, ia memotret preset model efektif ke
  `sessions/<id>/model-preset.json`. Anak mewarisi snapshot parent-nya
  (termasuk variant, temperature, options, fallback chain).
- Mengubah default global tidak mengubah sesi yang sedang jalan. Untuk
  mengubahnya: `/model-preset <nama>` (berlaku untuk delegasi berikutnya di
  sesi itu) atau reapply eksplisit. Lanjutan child dengan ID sama mengadopsi
  setelan baru parent-nya; child yang sedang berjalan tidak diganggu.
- TUI: `/model-preset` membuka manager 3 tingkat ala slim (daftar preset →
  susunan agen → edit agen: model → variant → temperature → options JSON);
  `/model-preset <nama>` = fast path ke sesi ini. Sidebar menampilkan preset
  model sesi itu + default global bila berbeda.
- Tanpa restart: hot-reload via watcher config (ala slim v2
  `liveConfigRefresh`) — perilaku v1 slim yang butuh reload TIDAK diadopsi.

Preset bawaan v0.1 (sebagai file — bukan konstanta; model ID di dalamnya
adalah contoh yang bisa diedit, verifikasi ketersediaan lewat
`purrcode models`):

- `free` (default): free-first — sedapat mungkin lokal (Ollama) + tier gratis
  (OpenRouter free). Whisker/Tinker/Calico = model cepat-murah; Sphinx =
  model gratis terkuat yang tersedia; kursi Clowder = campuran model gratis
  berbeda. Tidak memaksa jalur berbayar apa pun.
- `local`: semua Ollama (offline penuh; tanpa panggilan provider luar).
- `premium`: flagship per peran (BYO key; butuh provider berbayar — dipilih
  eksplisit oleh user, bukan default). Sphinx = flagship terkuat;
  Whisker/Tinker = flagship cepat. (Meowstro ikut model sesi — pilih flagship
  lewat `/model` atau `model` di preset mode bila mau.)

- **Model mati / tidak dikenal:** preset yang merujuk model yang sudah tidak
  ada (diganti nama/pensiun provider) tidak boleh membuat sesi crash.
  Validasi di `purrcode doctor` + saat preset diterapkan: entri yang modelnya
  tidak dikenal → peringatan + jatuh ke frontmatter agen itu, lalu ke model
  sesi; kejadian dicatat di audit log. Model ID di file preset adalah contoh
  yang bisa diedit — ketersediaan diverifikasi saat setup, bukan dijanjikan
  dokumen.
- **Interaksi dua fallback chain:** rantai fallback per agen di preset model
  dicoba dulu (urutan tertulis); bila habis karena error teknis, rantai
  fallback provider (PRD F12) mengambil alih. Penolakan kebijakan
  (`ProviderRefusal`) menghentikan SEMUA rantai — tidak ada fallback yang
  jalan setelahnya (aturan §7.3).

## 4. Memory — default MD minimal, backend plugin

Prinsip (keputusan user 2026-11-11 00:42 WIB): **defaultnya minimal dulu** —
cukup file Markdown, seperti Hermes dan Muse App sendiri. Tanpa database
wajib, tanpa vector, tanpa service eksternal di default. Yang lebih bagus
datang belakangan sebagai plugin yang bisa dipasang dan dicopot tanpa merusak
yang sudah ada. Yang paling utama: **modular, dan jangan breaking change.**

Default v0.1 — `MarkdownMemory`:

```
~/.purrcode/MEMORY.md   # memori kurasi (fakta, keputusan, preferensi)
~/.purrcode/USER.md     # siapa user, apa yang penting buat dia
~/.purrcode/SOUL.md     # persona & nada PurrCode
.purrcode/MEMORY.md     # memori per proyek (menang atas global bila bentrok)
sessions/<id>.jsonl     # transkrip sesi (bukan recall — jejak audit + resume)
```

- `MEMORY.md` dimuat di setiap turn (dibatasi token, ala Hermes 2200/1375 karakter
  untuk konteks yang disuntik — angka persisnya di-tuning saat implementasi).
  Hot-reload via mtime (ala PicoClaw): edit manual langsung berlaku tanpa restart.
- Recall default = grep sederhana atas file MD (tanpa index). Cukup untuk
  v0.1; bila terbukti kurang, naik kelas lewat plugin, bukan dengan mengganti
  default.
- `/compact`: model merangkum sesi jadi ringkasan + fakta baru ke `MEMORY.md`,
  dengan instruksi fokus dari user; compaction **non-destruktif** (JSONL asli
  dipertahankan, ala pi: reserve/keep token).

Memory sebagai modul (bukan hardcode):

- Trait `MemoryBackend` di `purrcode-api` (v1, berversi): `remember(fact)`,
  `recall(query, limit) -> Vec<Fact>`, `forget(id)`,
  `export_markdown() -> String`. Core hanya bicara ke trait; tidak tahu
  backend apa di belakangnya.
- Config: `memory.backend = "markdown"` (default). Ganti backend = ganti satu
  baris config, tanpa ubah core, tanpa rebuild.
- Backend bawaan opsional `sqlite-fts`: SQLite FTS5 (pola PicoClaw/Hermes —
  tabel facts/decisions/people, decay ala ZeroClaw), diaktifkan eksplisit
  lewat config; file `memory.db` hanya ada bila backend ini aktif.
- Backend eksternal (plugin proses JSON-RPC — transport yang sama dengan
  plugin lain): kandidat yang user sebut — menmosyne, supermemory, hindsight —
  **belum dianalisis** (lisensi, API, dan cara pasangnya belum diverifikasi;
  dicatat sebagai kandidat, bukan janji). Masing-masing dipasang sebagai plugin
  memory; core tidak bergantung pada satu pun.
- Proyek ke-2 khusus memory (roadmap, belum diputuskan): bila suatu saat
  dibutuhkan engine memory yang lebih kompleks, ia dibangun sebagai proyek
  terpisah dengan protokol backend yang sama — dipasang sebagai plugin, core
  tetap tidak berubah.

Aturan anti-breaking-change:

1. **Format MD adalah kontrak stabil.** `MEMORY.md`/`USER.md`/`SOUL.md`
   tidak berubah format; backend baru DILARANG menulis ulang atau
   merestruktur file MD. Semua backend WAJIB bisa `export_markdown()`.
2. **Dual-write ke MD.** Fakta apa pun yang ditulis backend eksternal harus
   juga tercermin di `MEMORY.md` — jadi copot plugin pun memori tidak hilang.
3. **Trait berversi.** Perubahan trait = versi baru (`memory/v2`) + adapter
   dari v1; v1 tidak pernah diubah setelah rilis.
4. **Ganti backend tanpa migrasi paksa.** Data MD tetap sumber kebenaran;
   backend baru mulai dari impor MD; data lama tidak dihapus atau dipindah
   paksa.
5. **Session JSONL append-only berversi** — formatnya tidak berubah.

- Skill (`SKILL.md` + frontmatter `name`/`description`): hanya deskripsi dimuat
  di awal; isi penuh on-demand (standar agentskills.io, ala Claude Code).
  Discovery: `.purrcode/skills/` → `~/.purrcode/skills/` → bawaan.
  **Bawaan v0.1 (8):** `plan`, `review`, `git`, `debug`, `scaffold`,
  `frontend-design`, `webapp-testing`, `doctor` (lihat PRD F8).
  Delapan saja dengan sengaja: tiap deskripsi skill membebani ~100 token
  metadata saat startup — 28 skill ala Muse Code tidak perlu ditiru semuanya.
  **Aturan lisensi skill:** skill dokumen Anthropic (`docx`/`pdf`/`pptx`/
  `xlsx`) berlisensi source-available yang melarang karya turunan — bila
  PurrCode butuh skill dokumen, tulis dari nol, jangan turunkan dari sana.
  Skill contoh Anthropic lainnya Apache-2.0 (pola boleh dipelajari); skill
  Muse Code tidak berlisensi jelas (dari ekstraksi binary) — hanya pola,
  bukan salinan teks.
- Kurator skill otomatis (ala Hermes) **ditunda v0.3**; v0.1: `purrcode skill learn`
  manual + usulan dari agen yang harus disetujui user.

## 5. Plugin (Rust)

```yaml
# ~/.purrcode/plugins/<nama>/plugin.yaml
name: my-tool
version: 0.1.0
kind: tool            # tool | channel | provider | hook
transport: builtin    # builtin (feature Cargo) | process (JSON-RPC stdio)
provides:
  tools: [mytool]
hooks: [pre_tool_use, post_tool_use]
config: { ... }
```

- **Kontrak bersama** (ala `zeroclaw-api`): trait `Tool`, `Channel`,
  `ModelProvider`, `Plugin` di crate `purrcode-api` — satu-satunya permukaan
  yang plugin boleh sentuh. Microkernel: tanpa feature `agent-runtime` yang ada
  hanya kernel (config + provider + memory + CLI).
- **Dua transport.** (a) *Builtin:* plugin dikompilasi sebagai Cargo feature,
  di-link ke binary (pola ZeroClaw). (b) *Process:* plugin eksternal berjalan
  sebagai proses terpisah dan bicara JSON-RPC lewat stdio — protokol stabil
  dan berversi, runtime ter-pin oleh PurrCode (pola subagent DeepSeek Harness;
  **bukan** eksekusi PATH sembarangan, kredensial di-scrub dari env turunan).
  Transport (b) adalah jalan agar plugin bisa ditulis dalam bahasa apa pun
  tanpa me-link kode ke binary GPL.
- **Lifecycle hook:** `on_start`, `on_session_start`, `pre_tool_use`,
  `post_tool_use`, `on_stop` — fan-out ke semua plugin (ala slim `event()`).
  Hook `pre_tool_use` bisa mem-veto (dipakai enforcement keamanan, ala Claude Code).
- **Discovery:** `~/.purrcode/plugins/` + `.purrcode/plugins/`; manifest dibaca
  di startup; plugin rusak = dilewati dengan peringatan, bukan crash.
- **Mount/unmount:** `purrcode plugin list/enable/disable/install` (install dari
  path git lokal di v0.1; marketplace ditunda; WebUI v0.4 memberi UI-nya).
- **MCP:** tool eksternal via MCP client (bukan plugin runtime) — satu-satunya
  jembatan ke ekosistem TS/Claude Code.
- **Kapabilitas plugin (deklarasi di manifest, default deny):** plugin
  menyatakan apa yang ia butuhkan — `capabilities: { network: [host...],
  fs: [path...], secrets: [...] }`. Tanpa deklarasi = tanpa akses. Secret
  (token provider, kredensial) TIDAK PERNAH diteruskan ke plugin kecuali
  kapabilitas `secrets` dideklarasikan DAN user menyetujui saat install.
  Proses plugin berjalan dengan env yang di-scrub; pelanggaran kapabilitas
  = plugin dimatikan + masuk audit log.
- **Yang tidak dilakukan v0.1:** plugin WASM/Cranelift (beban ZeroClaw yang tidak
  perlu di v0.1), hot-reload plugin (restart daemon).

## 6. Channel

| Channel | Transport | Render |
|---|---|---|
| TUI (v0.1) | Ratatui, in-process | Card tool call, preview edit, `ask` picker, sidebar subagent, status bar |
| `--json` / `-p` (v0.1) | NDJSON stdout / one-shot | Event stream mentah |
| Telegram (v0.2) | Long-poll `getUpdates`, hand-rolled reqwest (tanpa SDK berat, ala ZeroClaw) atau teloxide | Pesan + chunking 4096 + tombol approval inline |
| WebUI (v0.4) | HTTP + SSE di atas event bus | Sesi, transcript, tool card, plugin manager, scheduler |
| Discord/dll (nanti) | Plugin channel | Sama: render event bus |

Channel mendaftarkan diri ke registry (pola PicoClaw `init()` + capability opt-in:
`typing`, `edit`, `reaction`, `streaming` — channel yang tidak mendukung
streaming menerima pesan utuh). Sesi per chat ID; `allow_from` fail-closed.

## 7. Keamanan, mode & guardrail (berlapis)

### 7.1 Preset = file, bukan kode (anti-hardcode)

Tidak ada satu pun mode yang di-hardcode di Rust. Semua mode adalah **preset**:
file JSONC berskema `preset/v1` yang dibaca saat startup. Code hanya
menginterpretasi skema; menambah mode = menambah file — sama seperti agen.

Discovery (menang yang paling spesifik): bawaan `presets/*.jsonc` →
`~/.purrcode/presets/*.jsonc` → `.purrcode/presets/*.jsonc`.
Pemilihan: flag `--preset <nama>` (menang atas segalanya) → config
`preset = "<nama>"` → default `tabby`. Perintah: `purrcode preset list` /
`purrcode preset show <nama>`.

Satu preset berisi: `{ schema, name, approval, guardrail, sandbox, tools,
model }` — yakni kombinasi tiga sumbu di bawah + tool set + model sesi
(Meowstro). Model per subagent TIDAK ada di sini — itu preset model (§3.5).
Skema berversi; perubahan skema = `preset/v2` + migrasi, bukan mengubah v1
(aturan anti-breaking-change §4).

### 7.2 Tiga sumbu (ortogonal — jangan disatukan jadi satu label)

- **Sumbu approval** — seberapa sering agen bertanya sebelum bertindak:
  `manual` (tanya semua) / `supervised` (tanya write/edit/exec di luar
  workspace) / `auto` (tanya hanya destruktif) / `yolo` (tidak bertanya).
  **Yolo hanya via flag eksplisit per-sesi, tidak pernah default, dan selalu
  dicatat di audit log.** Yolo soal *approval* saja — bukan soal konten.
- **Sumbu guardrail** — aturan konten & batasan yang ditegakkan agen:
  `strict` / `default` / `god` (lihat §7.3).
- **Sumbu sandbox** — pembatasan sistem: `confinement` (workspace +
  allowlist shell sempit, default v0.1) / `os` (Landlock/seccomp/Seatbelt,
  v0.3).

Preset bawaan v0.1 (tiga, sebagai file — bukan konstanta; nama display
bertema kucing (diputuskan user 2026-11-11 00:50 WIB):

| Preset | Sumbu approval | Sumbu guardrail | Sumbu sandbox | Cocok untuk |
|---|---|---|---|---|
| `carrier` (Strict) | `manual` | `strict` | `confinement` | Repo kerja/bersama, mesin yang tidak boleh disentuh sembarangan |
| `tabby` (Default) | `supervised` | `default` | `confinement` | Pemakaian sehari-hari — ini defaultnya |
| `oyen` (God) | `supervised` * | `god` | `confinement` | Lihat §7.3 |

\* `oyen` tidak otomatis `yolo`: god mode mematikan guardrail konten di sisi
agen, bukan approval. Mau tanpa tanya juga? Tambahkan `--yolo` eksplisit —
dua keputusan terpisah, dua flag terpisah.

### 7.3 Guardrail berlapis (agen → provider)

- **`strict` (Carrier):** guardrail paling ketat — tolak permintaan yang
  melanggar hukum/ilegal, tolak akses kredensial & secret, tolak egress
  jaringan ke host tak dikenal, tool set read + write terbatas workspace,
  exec hanya allowlist sempit, approval `manual`.
- **`default` (Tabby):** baseline legal — tindakan yang jelas ilegal ditolak
  (merusak sistem orang lain, mencuri kredensial, dsb.), tapi pekerjaan
  security user atas sistem & kode **sendiri** diizinkan (user adalah security
  engineer; audit, RE, dan tooling atas device sendiri adalah kerja sah).
  Approval `supervised`.
- **`god` (Oyen):** guardrail di sisi agen dimatikan —
  agen tidak menambah penolakan konten sendiri di atas apa pun. Bukan berarti
  tanpa aturan: (a) approval tetap mengikuti sumbu approval preset; (b) audit
  log tetap nyala dan tidak bisa dimatikan di mode apa pun; (c) opt-in eksplisit
  per sesi (`--preset oyen` + konfirmasi, tidak ditawarkan di onboarding);
  (d) **lapis provider tetap berlaku** — lihat di bawah.

**Lapis provider (ToS masing-masing, tidak bisa di-bypass harness).**
Mode guardrail di atas adalah aturan *agen*. Setiap provider menegakkan
kebijakannya sendiri — Claude (Anthropic), OpenAI, Google (Gemini), DeepSeek,
xAI, Mistral, Groq, Cerebras, OpenRouter (plus provider di baliknya), Kimi,
MiniMax — dan PurrCode tidak mengklaim tahu isi detail tiap ToS. Yang
ditegakkan harness:

- Penolakan provider disajikan apa adanya ke user (alasan + provider yang
  menolak), bukan disembunyikan sebagai output sehat (pilar harness Muse §11).
- **Fallback chain tidak pernah dipakai untuk menghindar dari penolakan
  kebijakan.** Provider menolak karena kebijakan ≠ provider error/kapasitas.
  Adapter menormalisasi penolakan jadi event `ProviderRefusal { provider,
  reason }`; fallback hanya jalan untuk error teknis (timeout, 5xx, habis
  kuota). Mencoba provider lain supaya lolos dari penolakan yang sama adalah
  pelanggaran ToS provider itu — harness tidak melakukannya.
- Model lokal (Ollama / LM Studio) tidak punya lapis provider: di sanalah god
  mode bermakna penuh, dan tanggung jawab sepenuhnya di tangan user yang
  menjalankannya di mesin sendiri.

- **Rule engine:** `deny > ask > allow`; wildcard untuk Bash; bare-name deny
  menghapus tool dari konteks model (ala Claude Code).
- **Subagent hanya boleh mempersempit policy parent** (ala ZeroClaw): child
  dari parent `strict` tidak bisa naik ke `god`; child dari parent `god`
  tetap terikat approval & sandbox parent. Cron job tidak boleh mengelola cron.
- **Sandbox:** v0.1 = workspace confinement + allowlist shell sempit + audit log.
  v0.3 = Landlock/seccomp (Linux), Seatbelt (macOS). Jangan pernah klaim
  `file_safety` sebagai security boundary (pelajaran Hermes).
- **Secret:** kredensial tidak pernah dikirim ke LLM tanpa filter `[FILTERED]`
  (ala PicoClaw); token disimpan di `~/.purrcode/credentials` (0600), bukan di
  config yang bisa terbaca.
- **Audit log:** `~/.purrcode/audit.jsonl` — semua tool call, approval, spawn,
  penolakan, dan pilihan mode/preset. Tidak bisa dimatikan, di mode apa pun.
  **Rotasi:** file dirotasi per ukuran (batas di-tuning saat implementasi) +
  dikompresi (`audit-<tanggal>.jsonl.gz`); retensi minimal 30 hari. Rotasi
  tidak pernah menghapus file yang sedang ditulis dan tidak pernah diam-diam
  membuang catatan — `purrcode audit` menampilkan dan memverifikasi arsip.
- **Cron (v0.3):** job cron berjalan sebagai Meowstro di sesi khusus, dengan
  preset yang aktif saat job dibuat — tapi **tidak pernah `oyen` dan tidak
  pernah `yolo`** (eksekusi tanpa pengawasan tidak boleh tanpa guardrail dan
  tanpa persetujuan). Aksi tulis yang dibutuhkan job harus masuk allowlist
  yang dideklarasikan saat job dibuat; apa pun di luar allowlist → job gagal
  + memberi tahu user, bukan bertanya di tengah malam. Job tidak boleh
  mengubah/melewati preset-nya sendiri.

## 8. Data & direktori

```
~/.purrcode/
  purrcode.jsonc        # config global (berlapis, merge ramah-gagal)
  credentials           # 0600, token provider
  MEMORY.md SOUL.md USER.md
  presets/<nama>.jsonc        # preset mode (preset/v1): carrier/tabby/oyen + custom
  model-presets/<nama>.jsonc  # preset model (model-preset/v1): free/local/premium + custom
  sessions/<hash-cwd>/<id>.jsonl
  sessions/<id>/subagents/<subid>/{brief.md,result.md}
  sessions/<id>/workgraph.json
  sessions/<id>/preset.json        # snapshot preset mode efektif sesi itu
  sessions/<id>/model-preset.json  # snapshot preset model efektif sesi itu
  memory.db             # hanya bila backend sqlite-fts aktif (opsional)
  audit.jsonl           # rotasi per ukuran, lihat §7
  plugins/<nama>/plugin.yaml
  skills/<nama>/SKILL.md
  agents/<nama>.md
.purrcode/               # per proyek: agents, skills, plugins, presets,
                         #   model-presets, SYSTEM.md, MEMORY.md
```

Format config: JSONC (komentar diizinkan), merge system → global → proyek,
guard prototype pollution, migrasi legacy no-clobber + backup (ala `omo.jsonc`).

## 9. Implementasi v0.1 (Rust)

```
src/  (workspace Cargo — rantai crate satu arah, ala OpenHuman, ditegakkan CI)
  purrcode-api/       # trait Tool/Channel/ModelProvider/Plugin (kontrak bersama)
  purrcode-core/      # event bus, agent loop, session store
  purrcode-provider/  # adapter: Anthropic, OpenAI, Gemini, DeepSeek, xAI, Mistral,
                      #   Groq, Cerebras, OpenRouter, Kimi, MiniMax, custom endpoint,
                      #   Ollama/LM Studio; fallback chain + cooldown
  purrcode-tools/     # 12 tool inti + registry
  purrcode-orchestrator/ # work graph, task(), registry subagent
  purrcode-memory/    # trait MemoryBackend + MarkdownMemory default + compaction
  purrcode-plugin/    # manager + hooks + transport process JSON-RPC
  purrcode-channel/   # registry + TUI (ratatui) + json/print
  purrcode-approval/  # rule engine + audit
  purrcode-config/    # JSONC berlapis + onboarding
  purrcode-service/   # systemd/OpenRC install (ala zeroclaw service install)
  purrcode-web/       # v0.4: HTTP + SSE + WebUI
  purrcode-cli/       # binary: clap (agent/gateway/web/cron/skills/onboard/plugin/service)
```

- **Stack:** tokio (async), ratatui + crossterm (TUI), rusqlite (session store
  + plugin sqlite-fts opsional),
  reqwest + rustls (bukan OpenSSL), serde/serde_json, clap, tracing.
- **Build:** `[profile.release]` ala ZeroClaw — `opt-level="z"`, `lto="fat"`,
  `codegen-units=1`, `strip`, `panic="abort"`; musl statis via `cargo zigbuild`;
  feature-gating agresif (default hanya channel + provider yang dipakai);
  `cargo deny` untuk lisensi dependensi (GPL-3.0: hanya dependensi kompatibel GPL).
- **Pengujian:** skenario PRD §4.5 (delegasi 3 langkah + interupsi + resume via ID);
  golden test untuk `edit` (hash-anchored, preview); test approval (deny menang,
  timeout → deny); test startup (model < 64K ditolak dengan pesan jelas);
  test penegakan preset (Meowstro memanggil `write` → ditolak
  `tool_not_in_preset`); test isolasi preset model (ubah default tidak mengubah
  sesi jalan); test fallback (penolakan kebijakan tidak memicu provider lain);
  test rotasi audit (file lama terkompresi, tidak ada catatan hilang).
- **Risiko Rust yang dicatat jujur:** compile lebih lambat dari Go; plugin
  in-process lebih sulit (dijawab transport process JSON-RPC); TUI ratatui
  matang tapi kustomisasi card butuh kerja manual.

## 10. Roadmap teknis

| Fase | Isi | Kriteria selesai |
|---|---|---|
| 0.1a | Scaffold + core (bus, loop, session, provider Ollama) + `-p` | One-shot jalan end-to-end |
| 0.1b | TUI + 12 tool + approval + memory MD default | Skenario PRD §4.5 lolos |
| 0.1c | Meowstro + The Clowder + plugin manager + `service install` | v0.1 rilis, binary ≤ 25 MB |
| 0.2 | Gateway Telegram + approval inline + `allow_from` | Bot Telegram jalan di HP user |
| 0.3 | Cron + kurator skill + sandbox OS | Otonom harian tanpa intervensi |
| 0.4 | WebUI khusus (`purrcode web`): sesi, plugin manager, scheduler | WebUI lokal jalan, TUI tetap utama |

Catatan: **tugas saat ini hanya sampai dokumentasi & arsitektur (dokumen ini).
Tidak ada kode yang ditulis pada tahap ini** — keputusan user 2026-11-11.

## 11. Keputusan yang dicatat

1. **Rust v0.1** (keputusan user 2026-11-11; Go sebagai alternatif yang dievaluasi,
   bukan yang dipilih). Alasan Rust: binary statis kecil, sandbox OS natural,
   bukti codex/zeroclaw/openhuman; risiko compile lambat diterima.
2. **GPL-3.0** (keputusan user 2026-11-11). Konsekuensi: dependensi harus
   kompatibel GPL; plugin eksternal lewat proses JSON-RPC agar tidak me-link
   kode proprietary ke binary.
3. **Ratatui** untuk TUI (bukan renderer custom ala pi/omp, bukan Bubble Tea —
   itu Go).
4. **Plugin dua transport** (builtin feature Cargo + process JSON-RPC), bukan WASM,
   bukan vendor Cordis.
5. **Memory default MD minimal; modular, tanpa breaking change** (keputusan user
   2026-11-11 00:42 WIB). Default = file MD (`MEMORY.md`/`USER.md`/`SOUL.md`)
   ala Hermes & Muse App, tanpa vector DB v0.1; recall default = grep atas MD.
   FTS5, vector, dan backend eksternal = plugin di belakang trait
   `MemoryBackend` v1 (`memory.backend = "markdown"` default; `sqlite-fts`
   bawaan opsional). Kandidat backend eksternal (belum dianalisis): menmosyne,
   supermemory, hindsight. Proyek ke-2 khusus memory yang lebih kompleks adalah
   opsi roadmap, bukan bagian dari v0.1 — bila ada, ia dipasang sebagai plugin
   lewat protokol backend yang sama. Aturan anti-breaking-change §4 mengikat
   semua backend. → ARCHITECTURE §4, PRD F7.
6. **Session store: JSONL + SQLite** (bukan salah satu saja): JSONL append-only
   = jejak audit + resume; SQLite = tabel `session`/`turn`/`tool_call`.
   Recall FTS5 bukan bagian session store — hanya lewat plugin `sqlite-fts`.
7. **Subagent: continuation ID + handoff file + registry** (bukan prompt-only).
8. **Bahasa balasan adaptif** — mengikuti bahasa user & model (PRD §2.7).
9. **WebUI v0.4** sebagai channel tipis di atas event bus yang sama, bukan core kedua.
10. **6 pilar harness Muse Code sebagai aturan perilaku** (exhaustive checklist,
    independent oracle, anti-run-ahead, silent collateral reversion, kegagalan
    eksternal tampak sebagai cacat deliverable, isolasi multi-agent) — aturan,
    bukan kode; sumber: `references/muse/ANALYSIS.md` §3.
11. **Skill description sebagai kontrak routing + preset sebagai unit komposisi
    tool + event protocol JSON berversi ala MSP** (70 methods / 45 notifications /
    37 errors sebagai patokan bentuk, bukan wire) — sumber: `references/muse/ANALYSIS.md` §5, §10.
12. **Meowstro tidak menulis kode (Set Clowder, keputusan user 2026-11-11
    00:38 WIB).** Meowstro = petinggi/perwakilan user: paham codebase, susun
    rencana, delegasi, verifikasi — implementasi selalu ke Tinker dkk. Tool set
    Meowstro read-only + delegasi (`read`, `grep`, `glob`, `find`, `todo`,
    `ask`, `task`); `write`/`edit`/`bash` tidak ada di preset-nya dan ditolak
    runtime. Tujuannya menjaga konteks utama tetap ramping (hanya peta +
    ringkasan + keputusan); detail tinggal di subagent. → PRD §4.5 butir 6,
    ARCHITECTURE §3.4, DESIGN ADR-002.
13. **Preset & guardrail berlapis (keputusan user 2026-11-11 00:47 WIB).**
    Preset = file `preset/v1` (bukan hardcode); tiga sumbu (approval ×
    guardrail × sandbox); Carrier/Tabby/Oyen; lapis ToS provider tidak bisa
    di-bypass — fallback tidak retry atas penolakan kebijakan.
    → `ARCHITECTURE.md` §7, `PRD.md` §4.6, `DESIGN.md` ADR-003.
14. **Preset model ala slim (keputusan user 2026-11-11 00:52 WIB).**
    Terpisah dari preset mode: `free` (default), `local`, `premium`, + custom
    (nama file bebas). Hanya setelan inferensi per subagent; Meowstro ikut
    model sesi; binding per sesi (snapshot), tanpa restart.
    → `ARCHITECTURE.md` §3.5, `PRD.md` §4.7, `DESIGN.md` ADR-004.
15. **Tanpa default model hardcode — wajib di-set user lebih dulu (keputusan user 2026-11-11).**
    Tidak ada model default hardcode yang berjalan diam-diam. User wajib mengonfigurasi
    provider & model lebih dulu via `purrcode-ai onboard` atau file config `purrcode.jsonc`.
    Mendukung custom endpoint / proxy OpenAI-compatible (seperti Magpie `http://127.0.0.1:3425/v1`,
    9router, atau OmniRoute ala opencode `opencode.json` dan omp `models.yml`) dengan field
    `baseUrl`, `apiKey`, `models` (`contextWindow`, `maxTokens`, `reasoning: true/false`,
    `variants: reasoningEffort`). Bila dijalankan tanpa model yang dikonfigurasi, sistem menolak
    jalan di mode non-TUI dengan pesan error jelas, atau membuka wizard onboarding di mode interaktif.
    → `PRD.md` §4.1 F12, §7 butir 8; `DESIGN.md` §7.
