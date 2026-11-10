# PurrCode AI — Analisis Referensi (00)

Tanggal analisis: 2026-11-11 WIB. Clone: `references/` (total ~2,0 GB, **jangan di-commit** —
lihat `.gitignore`). Semua repo di-clone shallow, read-only, tanpa build/install/push.
Tiap repo punya `references/<nama>/ANALYSIS.md` lengkap (±140–460 baris) — dokumen ini
sintesisnya, bukan penggantinya.

**Keputusan user 2026-11-11 00:24 WIB (revisi):** bahasa **Rust**, lisensi **GPL-3.0**,
bahasa balasan adaptif, provider terkenal + custom endpoint, WebUI khusus v0.4,
tugas saat ini dokumentasi & arsitektur saja (bukan kode). Paragraf §2.7 dan §4
di bawah sudah direvisi mengikuti keputusan ini; draf v0.1 yang mengusulkan Go
sudah tidak berlaku.

## 1. Inventaris terverifikasi

| Repo | HEAD (tanggal) | Bahasa | Lisensi | Ukuran clone | Peran di PurrCode |
|---|---|---|---|---|---|
| openai/codex | `2df4657` (2026-11-10) | Rust | Apache-2.0 | 147 MB | Sandbox + approval + subagent |
| can1357/oh-my-pi | `eda28ef` (2026-11-10) | TS + Rust | MIT | 316 MB | TUI + tools + extension/marketplace |
| earendil-works/pi | `b30d54b` (2026-11-10) | TS | MIT | 74 MB | Core minimal: loop, session JSONL, extension |
| sst/opencode | `7aa3be1` (2026-11-10), v2.2.7 | TS | MIT | 244 MB | Server + SSE + plugin in-process |
| anthropics/claude-code | — (dokumen publik saja) | — | Proprietary | — | Subagent-md, SKILL.md, hooks, permission |
| alvinunreal/oh-my-opencode-slim | `10fcee4` (2026-11-09) | TS | MIT | 44 MB | Pantheon 7 agen + mix model |
| deepseek-ai/deepseek-harness | `fcc0ad4` (2026-11-04), v0.2.2-rc.1 | TS | MIT | 238 MB | "Everything is a Plugin" (Cordis) |
| code-yeongyu/oh-my-openagent | `42d5746` (2026-11-11), v5.1.67 | TS + Rust | Sustainable Use v1.0 * | 206 MB | Continuation ID + memory git-backed |
| NousResearch/hermes-agent | `5d1435e` (2026-11-10) | Python | MIT | ~398 MB | Gateway Telegram + cron + skill self-improving |
| tinyhumansai/openhuman | `b14d6b3` (2026-11-09) | Rust | GPL-3.0 * | 134 MB | Rantai crate + Memory Tree + approval |
| sipeed/picoclaw | `bbf6893` (2026-08-19) | Go | MIT | 42 MB | Patokan single-binary Go ringan |
| zeroclaw-labs/zeroclaw | `6877da5` (2026-11-10) | Rust | Apache-2.0 + MIT | 121 MB | Microkernel + supervised fail-closed + service install |
| Muse (App + Code) | — (zip user 2026-11-11, Muse Code v1.4.14 menurut ekstraksi) | — | Proprietary / tidak tercantum * | 8,2 MB (ekstrak) | Harness perilaku (6 pilar) + MSP + skill routing |

\* **Batas lisensi — baca dulu sebelum meniru:**
- `oh-my-openagent` berlisensi **Sustainable Use License v1.0 (non-komersial)** — pola boleh
  ditiru, kode mentah **tidak boleh disalin**.
- `openhuman` **GPL-3.0** — menyalin/me-link kodenya membuat PurrCode wajib GPL-3.0.
  Aman: re-implementasi pola secara clean-room, atau interaksi arms-length via JSON-RPC.
- `claude-code`: repo publiknya **bukan open source** ("© Anthropic, All rights reserved").
  Yang dipakai hanya dokumentasi publik + standar terbuka SKILL.md/agentskills.io.
  Source-map bocor v2.1.88 yang beredar **dilarang dipakai** (DMCA).
- **Muse (App + Code): lisensi tidak tercantum dalam materi yang dianalisis**
  (Muse Code disebut "Meta AI internal" di README ekstraksi; Muse App proprietary).
  Yang diambil hanya pola/arsitektur + parafrase aturan perilaku — **bukan kode,
  bukan teks prompt mentah**. Provenance ekstraksi binary tidak terverifikasi
  independen; detail lengkap di `references/muse/ANALYSIS.md` §0.
- `deepseek-harness` masih **developer preview** — README sendiri memperingatkan
  *compatibility-breaking changes*, SAFETY.md: belum diaudit, bukan production-ready.
- Angka bintang GitHub sebagian tidak terverifikasi dari sandbox (API rate limit) dan
  dicatat sebagai tidak tersedia di tiap ANALYSIS.md — jangan dikutip sebagai fakta.

Koreksi asumsi awal yang penting (semuanya terverifikasi dari clone):
1. **opencode v2.2.7 sudah bukan Go + Bubble Tea.** 0 file `.go`; 5.340 file TS di
   `packages/`, server TypeScript di atas Effect, TUI OpenTUI + SolidJS.
2. **oh-my-opencode-slim: "Pantheon 7 agen" sudah kedaluwarsa.** Librarian di-merge ke
   Explorer (alias legacy); yang hidup: Orchestrator, Explorer, Oracle, Designer, Fixer,
   Observer (default off), Council + Councillor dinamis per seat.
3. **DeepSeek subagent bukan via PATH.** Claude Code memakai Agent SDK resmi dengan runtime
   ter-pin; Codex men-spawn wrapper package-local — keduanya one-shot, kredensial di-scrub.
4. **OpenHuman `vendor/` kosong di clone shallow** (submodule belum di-checkout): klaim soal
   transport Telegram / engine memory / TinySecurity di ANALYSIS.md ditandai
   "tidak terverifikasi dari clone ini". Perlu `git submodule update --init` bila didalami.
5. **Angka binary/RAM PicoClaw (~10 MB) dan ZeroClaw (~3,4 MB) tidak terverifikasi dari
   clone** (build dilarang dalam tugas ini). Yang dijadikan patokan adalah *teknik* build-nya,
   bukan angkanya. README PicoClaw sendiri mengoreksi build terbaru jadi 10–20 MB.

## 2. Temuan lintas repo — dan keputusan PurrCode

### 2.1 Subagent tidak hilang konteks (inti permintaan user)

| Mekanisme | Sumber | Kekuatan | Kelemahan |
|---|---|---|---|
| Disiplin prompt saja (brief ber-path/line, format output terstruktur, "amend the same child, never respawn", `task_revive`, board + wake scheduler) | oh-my-opencode-slim | Murah, tanpa infrastruktur | **Rapuh** — hanya janji model; tanpa lapis teknis |
| **Continuation ID `ses_...`** | oh-my-openagent | Teknis, bukan janji: child dilanjutkan, bukan di-respawn | Butuh session store yang mendukung resume |
| Child session + follow-up via `sessionID` | opencode | Background mode, konteks diteruskan eksplisit | Konteks fresh default; penerusan harus eksplisit |
| `spawn_agent` + message board, warisi config induk, batas kedalaman | codex | Config konsisten parent→child | Board = komunikasi, bukan konteks bersama |
| Worktree terisolasi + hasil tervalidasi skema | oh-my-pi | Hasil child terverifikasi sebelum dipakai | Berat (worktree per delegasi) |
| Agen = file `.md` + frontmatter, kembali sebagai ringkasan | claude-code | Sederhana, transparan, tanpa nesting | Ringkasan bisa kehilangan detail |

**Keputusan PurrCode:** gabungkan tiga lapis — (a) **continuation ID** (`purr_<id>`,
resume-first, respawn dilarang); (b) **handoff file** di session dir
(`brief.md` masuk + `result.md` keluar, jadi konteks child bisa dibaca ulang kapan saja);
(c) **registry subagent di orchestrator** (id, parent, status, ringkasan terakhir).
Disiplin prompt ala slim dipakai sebagai lapis keempat, bukan satu-satunya.
Detail kontraknya di `ARCHITECTURE.md` §3.

### 2.2 Memory — siapa yang punya, siapa yang tidak

- **Tidak ada memory bawaan:** oh-my-opencode-slim (grep = nol), deepseek-harness (hanya
  session JSONL v0–v4 + compaction + KV; "long-term memory" satu-satunya hit adalah
  komentar Ralph loop). Ketiadaan ini membenarkan PurrCode membangun memory sebagai
  **plugin inti**, bukan menumpang.
- **Memory serius:** oh-my-openagent (git-backed Markdown, refleksi, dream pass, recall
  kibitzer, sync, agreements, kartu orang); hermes (SOUL.md + MEMORY.md + USER.md,
  FTS5, nudge tiap 10 turn, kurator skill 7/14/30 hari); openhuman (Memory Tree =
  pohon namespace `source:<kind>` + `agent:<id>`, confinement `Reach::subtree`);
  picoclaw (3 lapis: JSONL sesi + Seahorse SQLite FTS5 trigram + MEMORY.md hot-reload);
  zeroclaw (SQLite + auto-save, dedup/decay/hygiene, policy gate); codex (AGENTS.md +
  rollout JSONL + compaction via model); claude-code (MEMORY.md + CLAUDE.md hierarkis +
  `/compact` dengan instruksi fokus).

**Keputusan PurrCode:** 3 lapis ala PicoClaw, tanpa vector DB di v0.1 —
(1) session JSONL append-only, (2) SQLite FTS5 untuk recall, (3) `MEMORY.md` +
`SOUL.md` + `USER.md` ala Hermes/Claude sebagai memori yang bisa dibaca manusia.
Skill: format `SKILL.md` + frontmatter (standar terbuka agentskills.io), dimuat on-demand;
kurator skill ala Hermes ditunda ke v0.3 (default: skill ditulis manusia / `purrcode skill learn`
manual, bukan otomatis).

### 2.3 Plugin — "Everything is a Plugin" dalam versi yang ringan

- **Cordis (deepseek-harness):** plugin = Service, context = repositori service ber-key
  stabil (`ctx.tools`, `ctx.llm`, …), dependensi via `inject`, event bertipe, registrasi
  reversibel via `ctx.effect()`. Komposisi deklaratif berlapis: profile → bundle →
  patch. Ini model plugin paling matang — tapi **jangan vendor Cordis**; ambil polanya
  (service-key + inject + efek reversibel) dalam ukuran Rust.
- **opencode:** plugin in-process (`Plugin.define` + hook domain agent/model/provider/
  tool/session/event/permission), aturan satu-copy-Effect.
- **pi / oh-my-pi:** extension factory TS (discovery `.pi/extensions` + `~/.pi/agent/
  extensions`, hot-reload), marketplace format Claude Code (`.omp-plugin/marketplace.json`
  + lock file), hooks/skills/custom-tools/MCP.
- **hermes:** platform = plugin via `plugin.yaml` + channel directory (refresh 5 menit).
- **zeroclaw:** microkernel — tanpa feature `agent-runtime` yang ada hanya kernel;
  kontrak bersama di `zeroclaw-api` (trait `Channel`, `ModelProvider`, `Tool`); semua
  opsional = Cargo feature.
- **picoclaw:** registry channel via `init()` + capability interface opt-in (typing/edit/
  reaksi/streaming via type assertion).

**Keputusan PurrCode:** plugin v0.1 = **trait Rust + manifest `plugin.yaml`**,
discovery `~/.purrcode/plugins/` + `.purrcode/plugins/`, lifecycle hook
(`on_start`, `on_session_start`, `pre_tool_use`, `post_tool_use`, `on_stop`),
dua transport: builtin (feature Cargo) + proses eksternal JSON-RPC stdio.
MCP dipakai sebagai jembatan tool eksternal (bukan plugin runtime).
WASM/Cranelift ditunda — beban Zeroclaw yang tidak perlu di v0.1.

### 2.4 TUI — jangan tulis renderer dari nol

- codex: Rust `ratatui` + `crossterm` di atas app-server JSON-RPC.
- oh-my-pi / pi: **renderer diferensial custom** (bukan Ink/React) — mahal, jangan ditiru.
- opencode v2: OpenTUI + SolidJS, TUI sebagai client tipis tanpa logika bisnis.
- Pola bersama yang benar: **TUI = client tipis di atas engine + event bus** (opencode:
  server + REST/SSE + bus event; codex: app-server JSON-RPC). Satu engine, banyak surface.

**Keputusan PurrCode:** Rust + **Ratatui** — TUI matang yang tidak perlu
ditulis ulang — sebagai client tipis di atas core daemon + event bus internal.
Satu engine yang sama dipakai TUI (v0.1), Telegram (v0.2), dan WebUI (v0.4).

### 2.5 Satu engine, banyak surface (pola opencode + claude-code)

opencode: `cli → server (Effect) → core → ai`, REST + SSE di atas bus event.
claude-code: CLAUDE.md/skills/hooks/MCP identik di TUI, IDE, web, Slack, CI.
Hermes: 1 gateway, 25 platform, 1 memori.
**PurrCode:** `purrcode` daemon core (agent loop, tool registry, session, memory,
plugin, approval) + channel sebagai plugin tipis (TUI, Telegram, lalu Discord/dll).
Channel tidak punya logika agen sendiri — hanya render + input.

### 2.6 Keamanan — tiru yang paling ketat, hindari default yolo

- **Tiru:** codex (approval enum 4 tingkat + `SandboxMode`, bwrap+Landlock+seccomp Linux,
  Seatbelt macOS, Windows Sandbox); zeroclaw (supervised default, delegasi default
  `Forbidden`, `OnNoApprover::Deny` fail-closed, timeout approval 120 dtk, audit log,
  allowlist shell sempit, subagent hanya boleh mempersempit policy parent);
  openhuman (approval gate, posture Standard/FlowStrict/Forced, expiry 10 menit,
  fail-closed, autonomy default OFF); claude-code (hooks sebagai enforcement
  deterministik di luar model, rule engine deny > ask > allow, bare-name deny
  menghapus tool dari konteks model).
- **Hindari:** oh-my-pi default `yolo` tanpa sandbox OS; pi tanpa permission system
  bawaan ("project trust" diakui bukan security boundary); opencode tanpa sandbox nyata
  (hanya komentar di kode; permission-prompt satu-satunya keamanan); hermes
  `file_safety.py` yang diakui sendiri "NOT a security boundary"; deepseek-harness
  yang belum diaudit ("must not be treated as secure or production-ready").
- **Catatan sandbox:** sandbox OS-level codex (Landlock/seccomp) adalah target akhir;
  v0.1 PurrCode cukup dengan approval gate + workspace confinement + allowlist
  (ala picoclaw `restrict_to_workspace: true` default) + audit log. Sandbox OS
  masuk roadmap v0.3, bukan janji v0.1.

### 2.7 Bahasa & ukuran — Rust sebagai keputusan v0.1 (revisi 2026-11-11)

| Kandidat | Bukti dari referensi | Untuk PurrCode |
|---|---|---|
| **Rust** | codex + zeroclaw + openhuman: binary lebih kecil, sandbox OS lebih natural, `opt-level="z"` + LTO + strip + musl | **Dipilih v0.1 — keputusan user 2026-11-11.** Binary statis kecil, sandbox OS natural, bukti codex/zeroclaw/openhuman. Risiko diterima: compile lebih lambat, plugin in-process lebih sulit (dijawab transport process JSON-RPC). |
| Go | picoclaw: 273 rb baris Go, single binary, `CGO_ENABLED=0` + `ldflags -s -w`, SQLite pure-Go (`modernc.org/sqlite`), cross-compile amd64/arm64/riscv64/mipsle; opencode v1 dulu Go; riwayat Go user (BFR-WEBUI-GO, 9router-go) | Alternatif yang dievaluasi, tidak dipilih. Compile cepat, Bubble Tea matang — tapi user memilih Rust. |
| TypeScript/Bun | pi/omp/opencode-v2/dsh: ekosistem plugin + MCP terkaya, tapi runtime berat, distribusi besar (omp 316 MB) | Ditolak untuk core. Dipakai hanya sebagai referensi pola; MCP tetap didukung sebagai tool eksternal. |
| Python | hermes: 3,97 jt baris, monolit, butuh container untuk sandbox | **Ditolak** — sesuai preferensi user (bukan desktop app, bukan Python, ringan). Polanya (gateway, cron, skill) ditiru; bahasanya tidak. |

**Keputusan ini bisa dibatalkan user** — lihat PRD §7 (keputusan terbuka). Bila user memilih
Rust, arsitektur di `ARCHITECTURE.md` tetap berlaku (ganti Bab implementasi, bukan desain).

### 2.8 Ringkasan "tiru vs hindari" per repo

| Repo | Tiru (3 teratas) | Hindari |
|---|---|---|
| codex | Approval enum + sandbox OS; `apply_patch` streaming; sesi JSONL + compaction | 120+ crate, protocol generated 1.193 file, TUI 505 rb baris |
| oh-my-pi | Extension + marketplace; subagent worktree + validasi skema; tool tier approval | Distribusi 316 MB, default `yolo`, Rust native yang luas |
| pi | Pemisahan core/CLI/TUI; event-stream loop; kontrak tool TypeBox; session tree + compaction non-destruktif | TUI dari nol, ketiadaan permission core |
| opencode | Server + SSE + bus event; loop murni; registry tool snapshot; ruleset permission; subagent child-session | Kunci ke Effect 4, permission-prompt sebagai satu-satunya keamanan, 29+ package di awal |
| claude-code | Subagent `.md`; SKILL.md on-demand; hooks enforcement; MEMORY.md hierarkis | Konteks awal 30–45K token, ketergantungan model Anthropic |
| oh-my-opencode-slim | Pantheon + mix-model preset; routing `<Agents>`; board + `task_revive` | Anti-hilang-konteks prompt-only; 52-file task-session-manager; tanpa memory |
| deepseek-harness | Pola Cordis (service-key + inject + efek reversibel); profile/bundle/patch; subagent runtime ter-pin | Vendor Cordis mentah; API preview yang akan breaking; tanpa memory bawaan |
| oh-my-openagent | Continuation ID `ses_...`; memory git-backed + dream; config `omo.jsonc` berlapis | Lisensi non-komersial (jangan salin kode); plugin OpenCode yang rapuh |
| hermes-agent | Gateway + `plugin.yaml`; memory berlapis + FTS5; cron bahasa alami; kurator skill | Monolit Python 3,97 jt baris; guard yang diakui bukan boundary |
| openhuman | Rantai crate satu arah + CI; grammar `provider:model` + gate `LocalOnly`; Memory Tree + confinement | GPL-3.0; backend managed vendor; .deb berdependensi X11/libasound |
| picoclaw | Satu binary Cobra + channel registry; memory 3 lapis tanpa vector DB; fallback chain model; build tag opt-in | 143 dependensi Go ikut ter-link; `allow_from` kosong hanya warning; tanpa approval per-aksi; tanpa contoh systemd |
| zeroclaw | Microkernel + feature-gating; resep build ringan; supervised fail-closed + audit; `service install` + OpenRC | Mengira repo kecil dari angka binary (repo 1,5 jt baris Rust); plugin WASM; `unwrap()` di jalur produksi |

### 2.9 Muse — referensi ke-13 (dari dalam, bukan dari repo publik)

Sumber: `Muse.zip` user (2026-11-11) → `references/muse/` + `references/muse/ANALYSIS.md`.
Dua produk, jangan dicampur: **Muse Code** (CLI, v1.4.14 menurut ekstraksi binary)
dan **Muse App** (asisten personal, Skyla AI / Muse Spark 1.3, yang sedang berjalan).

Temuan yang mengubah desain PurrCode:

- **Prompt utama Muse Code hanya 83 baris.** Ketelitian datang dari harness di
  sekitarnya, bukan dari prompt panjang — ini membenarkan PurrCode yang
  menginvestasikan di kontrak + approval + verifikasi, bukan di prompt raksasa.
- **6 pilar harness** (parafrase): exhaustive checklist, independent oracle,
  anti-run-ahead, silent collateral reversion (`git status` setelah installer),
  kegagalan eksternal harus tampak sebagai cacat deliverable (bukan disembunyikan),
  isolasi multi-agent. Semuanya aturan perilaku — nol kode berat, langsung
  diadopsi sebagai aturan kerja PurrCode (ARCHITECTURE §11, DESIGN §8).
- **Skill description sebagai kontrak routing.** Tiap `SKILL.md` Muse Code
  membuka dengan `description:` panjang yang menulis kapan WAJIB dimuat dan
  kapan JANGAN — ini format yang diadopsi mentah-mentah untuk skill PurrCode.
- **Preset sebagai unit komposisi tool:** `native-basic` (14 tool),
  `miniswe` (todo gating), `ac_orch` (`subagent_spawn` + `task_stream`),
  `openai-apply-patch` (1 tool). Mode kerja = tool set.
- **MSP v1** (Muse Session Protocol): 70 methods, 45 notifications, 37 errors —
  protokol berversi + generated (jangan edit manual). PurrCode meniru bentuknya
  (event protocol JSON berversi), bukan wire-nya.
- **Approval melahirkan aturan berdurasi** (`session` / `localPersistent`) +
  recovery setelah restart; lifecycle subagent eksplisit
  (`idle/working/blocked/done/unknown`, Herdr) + peer messaging antar-sesi.
- **Struktur memory Muse App:** `MEMORY.md` kurasi + catatan harian +
  `people/`+`groups/` INDEX + alignment synthesis latar + proactivity edition
  (keputusan kirim/lewati dengan alasan eksplisit). Isi privat user TIDAK ditiru —
  hanya strukturnya.

## 3. Yang belum terjawab dari referensi (jujur)

1. OpenHuman: transport Telegram, engine Memory Tree, dan TinySecurity tidak bisa
   diverifikasi dari clone (submodule kosong). Perlu `git submodule update --init`.
2. Hermes: detail `tool_loop_guardrails` dan ambang trigger kompresi tidak terverifikasi.
3. Angka bintang GitHub sebagian tidak terverifikasi (rate limit API unauthenticated).
4. Angka LOC semua perkiraan (file count + `wc -l` per batch), bukan hitungan eksak.
5. Angka binary/RAM PicoClaw/ZeroClaw tidak terverifikasi (build dilarang dalam tugas ini).
6. Muse: provenance ekstraksi binary Muse Code tidak terverifikasi independen;
   tiga file harness ~1,1 MB (~18.500 baris) belum dibaca penuh; internal runtime
   Muse App (source/bobot) tidak teramati dan tidak diklaim.

## 4. Keputusan arsitektur yang diturunkan dari analisis ini

1. **Rust, single binary statis musl**, feature-gating ala ZeroClaw. → `ARCHITECTURE.md` §1, §9
2. Core daemon + event bus; TUI, Telegram, dan WebUI (v0.4) = channel tipis. → `ARCHITECTURE.md` §2, §6
3. Meowstro + The Clowder (Set Clowder, keputusan user 2026-11-11):
   Meowstro/Whisker/Sphinx/Calico/Tinker/Clowder — adaptasi Pantheon slim,
   Whisker menyerap Librarian. → `PRD.md` §4
4. Subagent: continuation ID + handoff file + registry. → `ARCHITECTURE.md` §3
5. Memory default MD minimal (MarkdownMemory) + SKILL.md on-demand; FTS5,
   vector, dan backend eksternal = plugin di belakang trait `MemoryBackend`
   v1. Aturan anti-breaking-change §4 mengikat semua backend.
   → `ARCHITECTURE.md` §4
6. Plugin: trait Rust + `plugin.yaml` + hooks; dua transport (builtin feature Cargo +
   process JSON-RPC). → `ARCHITECTURE.md` §5
7. Approval: supervised fail-closed default (ala ZeroClaw), audit log. → `ARCHITECTURE.md` §7
8. Free-first: BYO provider terkenal + custom endpoint + Ollama lokal, tanpa jalur
   berbayar. → `PRD.md` §2
9. **GPL-3.0**; bahasa balasan adaptif (mengikuti user & model); WebUI khusus v0.4
   sebagai channel tipis. → `PRD.md` §2.7, §4.4; `DESIGN.md` §4, §5
10. **6 pilar harness Muse Code sebagai aturan perilaku PurrCode** (checklist,
    oracle independen, anti-run-ahead, collateral reversion, kegagalan tampak).
    → `ARCHITECTURE.md` §11; `DESIGN.md` §8
11. **Skill description sebagai kontrak routing** (wajib/jangan dimuat) + preset
    sebagai unit komposisi tool + event protocol JSON berversi ala MSP.
    → `ARCHITECTURE.md` §5, §11
12. **Preset & guardrail berlapis** (keputusan user 2026-11-11 00:47 WIB):
    preset = file `preset/v1` (bukan hardcode); tiga sumbu (approval ×
    guardrail × sandbox); Carrier/Tabby/Oyen; lapis ToS provider tidak bisa
    di-bypass — fallback tidak retry atas penolakan kebijakan.
    → `ARCHITECTURE.md` §7, `PRD.md` §4.6, `DESIGN.md` ADR-003.
13. **Preset model ala slim** (keputusan user 2026-11-11 00:52 WIB):
    terpisah dari preset mode: `free` (default), `local`, `premium`, + custom
    (nama file bebas). Hanya setelan inferensi per subagent; Meowstro ikut
    model sesi; binding per sesi (snapshot), tanpa restart.
    → `ARCHITECTURE.md` §3.5, `PRD.md` §4.7, `DESIGN.md` ADR-004.
14. **Tinjauan ulang 2026-11-11 00:54 WIB:** §11 ARCHITECTURE diurutkan ulang
    1–14; cross-ref PRD §4.3 → §4.5; tree direktori + snapshot preset;
    aturan cron (tidak pernah `oyen`/`yolo`), kapabilitas plugin (default
    deny), rotasi audit (≥30 hari), model preset mati (fallback berlapis),
    interaksi fallback chain; TUI menampilkan preset aktif.
    → `DESIGN.md` ADR-005.
15. **Skill bawaan v0.1 (8):** `plan`, `review`, `git`, `debug`, `scaffold`,
    `frontend-design`, `webapp-testing`, `doctor` — ditulis sendiri, pola dari
    Muse Code (28 skill) + `anthropics/skills` (19 skill). Skill dokumen
    Anthropic (docx/pdf/pptx/xlsx) source-available, melarang turunan — bila
    butuh, tulis dari nol.
    → `PRD.md` F8, `ARCHITECTURE.md` §4, `DESIGN.md` ADR-006.
