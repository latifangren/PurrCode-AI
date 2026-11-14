# Rencana Komprehensif Arsitektur & Implementasi PurrCode WebUI
## Developer Cockpit & Full Backend Parity (Svelte 5 + Bun + Rust Axum)

> **File:** `docs/webui-plan.md`  
> **Status:** APPROVED WORKPLAN & ARCHITECTURAL SPECIFICATION  
> **Tanggal/Waktu Mulai:** Sabtu, 14 November 2026, 19:30 WIB  
> **Dasar:** PRD §4.4, DESIGN §4 & ADR-011–013, AUDIT_STATUS v0.4, Arahan User (2026-11-14)  
> **Cabang Kerja:** `dev-muse` (basis commit `c4ccb7d`)  
> **Author/Orchestrator:** Meowstro / Orchestrator  

---

## 1. Latar Belakang, Temuan & Pivot Strategis

### 1.1 Evaluasi WebUI v0.4 Lama (Batch B1–B8)
Implementasi WebUI pada v0.4 awal (`crates/purrcode-channel/src/web/html*.rs`) berhasil membuktikan arsitektur server Axum 0.8, multi-sesi, SSE live stream, dan kepatuhan ADR-011 (<800 baris/file). Namun, evaluasi langsung menunjukkan kelemahan mendasar:
1. **Tampilan Terlalu Minimalis ("AI Slop"):** Antarmuka berbasis string HTML/JS statis terasa kaku, minim visual feedback, dan tidak mencerminkan kenyamanan tools modern developer seperti Cursor, Linear, atau DeepSeek Harness.
2. **Ketiadaan Parity Fitur Backend:** Banyak fitur canggih yang sudah matang di backend Rust (`crates/purrcode-core` & `crates/purrcode-service`) belum bisa dikonfigurasi melalui web:
   - Manajemen Model & Provider (CRUD API keys, custom base_url, latency ping).
   - The Clowder Model Routing Matrix (pemetaan model per subagent Meowstro/Tinker/Whisker/Sphinx/Calico).
   - Aturan Kernel Landlock Sandbox (manajemen allowlist direktori Read-Only / Read-Write).
   - MCP Server Hub (manajemen server stdio/sse dan discovery schemas).
   - Visual Diff & Patch Viewer untuk approval file edit dan dynamic skill synthesis (`manage_skill`).

### 1.2 Keputusan Pivot (Arahan User 2026-11-14)
- **Framework UI:** Menggunakan purrcode-cockpit yang dibuat di Google AI Studio (React 19 + TypeScript + Tailwind CSS v4 + Motion + Lucide + Vite) yang diekstrak ke `webui/`.
- **Backend API & Server:** Express + Vite dev server middleware di `webui/server.ts`, dengan persistensi atomic JSON store (`AtomicStateStore`) dan streaming SSE OpenAI-kompatibel murni (`POST /chat/completions`) yang terhubung langsung ke provider lokal / Magpie.
- **Batasan Binary:** Constraint binary strict <= 25MB ditiadakan oleh user untuk memberi ruang pada fitur kokpit pengembang yang lengkap dan visual modern.
- **Status Eksekusi (2026-11-14 21:30 WIB):**
  - [x] Ingest & extract prototype ke `webui/`.
  - [x] Pembersihan artifak AI Studio (hapus `metadata.json`, ganti SDK Gemini proprietary dengan native `fetch` + `TextDecoder` OpenAI streaming).
  - [x] Konfigurasi `.env` & provider seed default ke endpoint lokal Magpie (`127.0.0.1:3425/v1`, model `antigravity/gemini-3.8-flash-tiered`).
  - [x] 14/14 test webui lulus (`bun test`), 0 error linter TypeScript (`bun run lint`).
  - [x] Penyatuan Single Port (Opsi 1): Backend Rust Axum mengekspos `GET /api/state` (`routes_state.rs`, <800 baris) dan menyajikan bundle statis `webui/dist/` langsung pada port 8080.
  - [x] 346/346 test Rust core lulus (`cargo test --workspace`), 0 clippy warning.
  - [x] WebUI aktif di LAN pada port tunggal: `http://0.0.0.0:8080` (dapat diakses via `http://192.168.100.40:8080`).
- **Relaksasi Pagu Ukuran Binary (≤25MB):** Pagu 25MB dilonggarkan menjadi ~35–45MB agar dapat menampung WebUI modern SPA yang kaya fitur, visual diff viewer, dan editor Markdown interaktif langsung di dalam single binary tanpa dependensi eksternal.
- **Single Static Musl Binary Tetap Dipertahankan:** WebUI frontend di-build menjadi aset statis (`dist/`) dan di-embed ke dalam binary Rust menggunakan `rust-embed` saat kompilasi release.
- **Kenyamanan Pengembangan (DX):** Mode development mendukung HMR instan via `bun dev` dengan reverse proxy otomatis ke backend Rust `:8080`.

---

## 2. Arsitektur Tri-Pane Developer Cockpit

WebUI PurrCode mengadopsi layout **Tri-Pane Cockpit** yang padat informasi, responsif, dan ergonomis:

```
+-------------------------------------------------------------------------------------------------------------------------+
| [🐾 PurrCode Cockpit]  Project: ~/Project/PurrCode-AI    Preset: [tabby ▾]   Model: [DeepSeek-V3 ▾]    [● SSE: 8ms] [⌘K]|
+-------------+---------------------------------------------------------------------------------------------+-------------+
| SIDEBAR     | MAIN CANVAS (Chat / Diff / Matrix / Settings)                                               | LIVE HUD    |
| (Collapsible|                                                                                             | (Inspector) |
|  [Ctrl+B])  | ┌────────────────────────────────────────────────────────────────────────────────────────┐  | CONTEXT     |
|             | │ 🤖 Meowstro (Orchestrator)                                                  14:23:01 │  | [██████░░░] |
| [💬 Chat]   | │ Task: Refactor WebUI server into modular Svelte endpoints.                             │  | 42.1k / 128k|
| [🧭 Matrix] | │ Delegating diff synthesis to Tinker.                                                   │  | 32% used    |
| [⚙ Prov]   | └────────────────────────────────────────────────────────────────────────────────────────┘  |             |
| [🛡 Land]   |   │                                                                                         | ACTIVE TEAM |
| [🧠 Memo]   |   ▼                                                                                         | • Meowstro  |
| [🔌 MCP]    | ┌────────────────────────────────────────────────────────────────────────────────────────┐  |   (Idle)    |
|             | │ 🛠️ Tinker (Fixer)                                                          14:23:04 │  | • Tinker ⚡ |
| SESSIONS    | │ ▾ [✓ Ran 2 tool operations in 184ms]                                                   │  |   (Active)  |
| • Sesi #12  | │   ├─ 🔍 read "src/web/server/routes.rs:1-80"                                           │  |             |
| • Sesi #11  | │   └─ 📝 edit "src/web/server/routes.rs"                                                  │  | LANDLOCK    |
| • Sesi #10  | ├────────────────────────────────────────────────────────────────────────────────────────┤  | ABI: v3     |
|             | │ UNIFIED DIFF: src/web/server/routes.rs                                                 │  | RO: 4 paths |
| CLOWDER     | │ 24  - async fn handle_index() -> Html<String>                                          │  | RW: 1 path  |
| • Meowstro  | │ 24  + async fn handle_index() -> impl IntoResponse                                     │  | Net: Strict |
| • Tinker ⚡ | ├────────────────────────────────────────────────────────────────────────────────────────┤  |             |
| • Whisker   | │ ⚠️ SUPERVISED APPROVAL REQUIRED                                          ⏱️ Auto-deny 24s│  | RECENT LOG  |
| • Calico    | │ Action: Apply patch to `src/web/server/routes.rs`                                      │  | 14:23:04    |
|             | │ [ Y : Approve (Enter) ]        [ N : Deny (Esc) ]        [ E : Inspect Full Diff ]     │  | Tinker ->   |
| [⚙ Config]  | └────────────────────────────────────────────────────────────────────────────────────────┘  | edit file   |
|             |                                                                                             | (allowed)   |
|             | ┌────────────────────────────────────────────────────────────────────────────────────────┐  |             |
|             | │ [💬 Ask Meowstro anything or run /command...                                         ] │  |             |
|             | │ [📎 Add File] [/plan] [/model]                                       [Send Prompt ⏎] │  |             |
|             | └────────────────────────────────────────────────────────────────────────────────────────┘  |             |
+-------------+---------------------------------------------------------------------------------------------+-------------+
| STATUS BAR: [Landlock ABI v3: ACTIVE]  [Preset: Tabby]  [Memory: Markdown MD]  [Clowder: 2 Active]   [RAM: 38MB]        |
+-------------------------------------------------------------------------------------------------------------------------+
```

### Palet Warna: Catppuccin Mocha
- **Base Background:** `#1e1e2e` | **Mantle (Sidebar/Panels):** `#181825` | **Crust (Header/Borders):** `#11111b`
- **Surface Cards:** `#313244` | **Surface Hover:** `#45475a` | **Border Separators:** `#585b70`
- **Text:** `#cdd6f4` | **Subtext:** `#a6adc8`
- **Semantics:** Mauve (`#cba6f7`, Meowstro), Blue (`#89b4fa`, Tinker), Peach (`#fab387`, Whisker), Green (`#a6e3a1`, Success/Landlock), Red (`#f38ba8`, Danger/Deny), Yellow (`#f9e2af`, Approval Warning).

---

## 3. Rincian Checklist Implementasi per Fase

```
Phase 0: Toolchain & Build Pipeline
  │
Phase 1: Backend REST & SSE Parity Endpoints
  │
Phase 2: Svelte 5 Core Shell & Design System
  │
Phase 3: Execution Timeline & Live Approval Cockpit
  │
Phase 4: Management Studio (Providers, Matrix, Landlock, Skills, MCP)
  │
Phase 5: Live Inspector HUD & Mobile Ergonomics
  │
Phase 6: Embed, E2E Verification & LAN Dogfooding
```

---

### Phase 0: Workspace Setup, Toolchain & Build Pipeline
**Tujuan:** Membangun fondasi workspace di dalam repo (`webui/`) dengan toolchain modern Bun + Vite + Tailwind CSS v4.

- [x] **0.1 Inisialisasi Workspace `webui/`:**
  - Setup package: React 19, TypeScript, Tailwind CSS v4, Motion, Lucide icons.
  - Setup `vite.config.ts` dengan HMR native mode dan alias `@` terkonfigurasi.
  - Setup TypeScript `tsconfig.json` ketat (0 error pada `bun run lint` / `tsc --noEmit`).
- [x] **0.2 Pembersihan & Standarisasi:**
  - Menghapus artifak Google AI Studio (`metadata.json`, folder aset upload).
  - Mengganti SDK proprietary Gemini dengan streaming client SSE murni OpenAI-kompatibel (`POST /chat/completions`) via native `fetch` + `TextDecoder`.
  - Konfigurasi `.env` & `.env.example` diarahkan ke endpoint lokal Magpie (`127.0.0.1:3425/v1`, model `antigravity/gemini-3.8-flash-tiered`).
- [x] **0.3 Verifikasi Baseline Toolchain:**
  - `bun install` & `bun run build` menghasilkan bundle teroptimasi (`dist/`) hanya dalam 287ms.
  - 14/14 unit & integration test frontend lulus (`bun test`).
  - `cargo test --workspace` (343 tests) tetap 100% lulus tanpa regresi.

---

### Phase 1: REST & SSE Control Plane Endpoints (`webui/server/`)
**Tujuan:** Menyediakan endpoint RESTful dan SSE real-time dengan persistensi atomic JSON store (`AtomicStateStore`).

- [x] **1.1 Provider & Model Management API:**
  - `GET /api/state` & `POST /api/providers`: Tambah / update konfigurasi provider (Base URL, API Key, Model default).
  - `DELETE /api/providers/:id`: Hapus provider.
  - `POST /api/providers/:id/test`: Ping latency test (memvalidasi koneksi dan mengembalikan latency ms).
- [x] **1.2 The Clowder Model Routing Matrix API:**
  - `PUT /api/routing`: Memperbarui pemetaan model per subagent (`Meowstro`, `Tinker`, `Whisker`, `Sphinx`, `Calico`).
- [x] **1.3 Landlock OS Sandbox & Network Shield API:**
  - `GET /api/state` (sandbox): Status ABI Landlock, daftar direktori Read-Only / Read-Write.
  - `POST /api/sandbox/paths`: Tambah path allowlist baru dengan validasi sanitasi path ketat (cegah path traversal).
  - `DELETE /api/sandbox/paths/:id`: Hapus aturan path sandbox.
  - `PATCH /api/sandbox/network`: Toggle proteksi network outbound.
- [x] **1.4 MCP Server Hub API:**
  - `POST /api/mcp/servers`: Daftarkan server MCP baru (stdio/sse).
  - `DELETE /api/mcp/servers/:id`: Hapus integrasi server MCP.
  - `POST /api/mcp/servers/:id/toggle`: Toggle aktif/non-aktif server MCP.
- [x] **1.5 Identity, Memory & Skills Management Studio API:**
  - `PUT /api/memory`: Menyimpan perubahan memori markdown dengan pencatatan audit log otomatis.
  - `POST /api/skills/manage`: Trigger tindakan `manage_skill` (toggle status, apply diff preview).
- [x] **1.6 Keamanan & Audit Log:**
  - Setiap mutasi POST/PUT/DELETE dicatat ke rolling audit log `store.state.sandbox.auditLog`.
  - Header keamanan: `nosniff`, `SAMEORIGIN`, `no-referrer`.

---

### Phase 2: Core Developer Cockpit Shell & Multi-Theme System
**Tujuan:** Antarmuka Tri-Pane yang presisi, performa tinggi, dan bebas glitch visual.

- [x] **2.1 Token Desain & Theme Registry:**
  - 10+ preset tema bawaan: Catppuccin Mocha, Gruvbox Dark, Nord, Tokyo Night, Dracula, Cyberpunk, Forest, Monokai, Moonlight, Solarized Dark.
  - Kustomisasi token tema langsung di UI (Primary, Accent, Canvas, Panels, Borders, Font Mono).
  - Cat Companion SVG maskot animasi dengan toggle visibility & status reaktif.
- [x] **2.2 Komponen Shell Tri-Pane:**
  - `TopBar.tsx`: Breadcrumb project, nama branch aktif, preset badge, SSE latency badge, shortcut Command Palette `[⌘K]`.
  - `Sidebar.tsx`: List sesi aktif (ganti sesi instan), badge role tim Clowder, navigasi tab (Chat, Routing, Providers, Sandbox, Memory, MCP, Cron, Appearance).
  - `TaskProgressBanner.tsx`: Indikator turn AI berjalan dengan hold timer dan progress counter.
- [x] **2.3 Command Palette Modal (`⌘K` / `Ctrl+K`):**
  - Shortcut navigasi cepat antar tab, ganti preset mode (`tabby`/`carrier`/`oyen`), dan manajemen sesi.

---

### Phase 3: Execution Timeline & Live Approval Cockpit
**Tujuan:** Area chat interaktif dengan streaming real-time, collapsible tool chains, dan kartu approval.

- [x] **3.1 Message & Agent Header Cards:**
  - Header badge agent aktif (`🐾 Meowstro`, `🛠️ Tinker`, `🔍 Whisker`).
  - Model yang digunakan, durasi pemikiran AI (*thinking duration*), dan timestamp.
- [x] **3.2 Collapsible Tool Execution Chains:**
  - Accordion interaktif untuk tool steps (`read`, `grep`, `edit`, `bash`, `mcp`) dengan status: Completed, Running, Pending, Denied.
- [x] **3.3 Interactive Diff & Patch Viewer (`DiffViewer.tsx`):**
  - Tampilan visual diff sintaksis (tambah/hapus baris) untuk perubahan kode sebelum diaplikasikan.
- [x] **3.4 Supervised Approval Card (`ApprovalCard.tsx`):**
  - Kartu persetujuan tindakan berisiko dengan countdown timer, tombol Approve / Deny, dan audit tracking.
- [x] **3.5 Sticky Prompt Dock (`PromptDock.tsx`):**
  - Input prompt dengan auto-resize textarea, toggle quick preset, pin file context, dan shortcut `Enter` / `Shift+Enter`.

---

### Phase 4: Full Management Studio Views
**Tujuan:** Seluruh kemampuan engine PurrCode dapat dikonfigurasi melalui WebUI.

- [x] **4.1 Providers & Models View (`ProvidersView.tsx`):**
  - Form penambahan provider baru (OpenAI, DeepSeek, Anthropic, Ollama, Custom).
  - Tombol uji latensi real-time (*Ping test*).
  - Switch model aktif per sesi.
- [x] **4.2 The Clowder Routing Matrix (`RoutingView.tsx`):**
  - Pemetaan model AI khusus untuk masing-masing agen (`Meowstro`, `Tinker`, `Whisker`, `Sphinx`, `Calico`).
- [x] **4.3 Landlock OS Sandbox & Security View (`SandboxView.tsx`):**
  - Daftar allowlist path filesystem (Read-Only / Read-Write) dengan proteksi direktori sistem.
  - Explorer audit log real-time dengan filter actor, operasi, dan outcome.
- [x] **4.4 Memory & Skills Studio (`MemoryView.tsx`):**
  - Tab editor markdown untuk `MEMORY.md`, `USER.md`, `SOUL.md`.
  - Manajemen skill dinamis (`manage_skill`) dengan preview diff sintesis runtime.
- [x] **4.5 MCP Server Hub (`McpView.tsx`):**
  - Monitoring server MCP yang terdaftar beserta status koneksi dan alat yang diekspos.
- [x] **4.6 Cron Scheduler (`CronView.tsx`):**
  - Penjadwal tugas background dengan input bahasa alami (*Natural Language Cron parser*) dan status job.
- [x] **4.7 Appearance Customizer (`AppearanceView.tsx`):**
  - Pemilihan tema, live preview palet warna, dan kustomisasi maskot.

---

### Phase 5: Live Inspector HUD
**Tujuan:** Panel samping kanan untuk telemetri turn, pemakaian context token, dan status engine.

- [x] **5.1 Telemetri & Context Token Gauge (`InspectorHud.tsx`):**
  - Gauge persentase kapasitas token context window yang terpakai.
  - Telemetri latensi SSE stream dan waktu eksekusi.
  - Rincian tool run aktif dan ringkasan audit log keamanan.
- [ ] **2.3 Komponen Shell Tri-Pane:**
  - `TopCommandBar.svelte`: Breadcrumb project, nama branch aktif, preset badge, SSE latency badge, tombol shortcut Global Palette `[⌘K]`.
  - `Sidebar.svelte`: List sesi aktif (bisa ganti sesi tanpa refresh), indikator Clowder team, tombol navigasi tab utama (Chat, Matrix, Providers, Sandbox, Memory, MCP, Cron).
  - `StatusBar.svelte`: Status Landlock ABI, active preset, backend memory type, estimasi idle RAM.
- [ ] **2.4 Command Palette Modal (`⌘K` / `Ctrl+K`):**
  - Pencarian cepat: ganti model, switch preset (`tabby`/`carrier`/`oyen`), buka sesi baru, navigasi tab.

---

### Phase 3: Execution Timeline & Live Approval Cockpit
**Tujuan:** Mengubah area chat generik menjadi timeline eksekusi AI yang transparan dan dapat dikontrol secara penuh.

- [ ] **3.1 Message & Agent Header Cards:**
  - Avatar badge agent yang sedang bertindak (`🐾 Meowstro`, `🛠️ Tinker`, `🔍 Whisker`).
  - Timestamp turn, penanda model yang digunakan, dan waktu komputasi pemikiran (thinking duration).
- [ ] **3.2 Collapsible Tool Execution Chains:**
  - Desain komponen `ToolStepTree.svelte`: Ringkasan satu baris yang bisa di-expand: `[✓ Ran 4 tool steps (312ms) ▾]`.
  - Di dalam accordion: daftar runtut pemanggilan tool (`read`, `grep`, `edit`, `bash`) beserta argumen singkat dan status keberhasilan.
- [ ] **3.3 Interactive Unified Diff Viewer:**
  - Tampilan split / unified diff dengan gutter warna merah (hapus) dan hijau (tambah).
  - Highlight sintaks otomatis.
  - Aksi langsung: `[Accept Change]` / `[Reject]`.
- [ ] **3.4 Supervised Approval Card (Fail-Closed Gate):**
  - Kartu konfirmasi interaktif saat mode `tabby` memerlukan persetujuan tool destruktif.
  - **Circular Countdown Timer:** Lingkaran timer SVG 30 detik (gradasi kuning ke merah) yang otomatis auto-deny jika waktu habis (fail-closed).
  - Shortcut keyboard cepat: `Y` (Approve), `N` (Deny), `E` (Lihat Diff Lengkap).
- [ ] **3.5 Sticky Prompt Dock:**
  - Textarea auto-resize dengan styling JetBrains Mono.
  - Autocomplete popover untuk slash commands (`/plan`, `/model`, `/preset`, `/skill`, `/clear`).
  - Context chip / attachment indicator untuk file yang sedang di-pin ke prompt.

---

### Phase 4: Configuration & Management Matrices (WebUI Studio)
**Tujuan:** Memberikan antarmuka visual lengkap untuk semua parameter teknis yang sebelumnya tersembunyi di file konfigurasi.

- [ ] **4.1 Provider & Models Studio (`/settings/providers`):**
  - Grid card provider aktif (OpenAI, Anthropic, DeepSeek, Ollama, Custom URL).
  - Form penambahan provider baru dengan toggle mask API key.
  - Tombol **"Test Latency Ping"** interaktif dengan indikator milidetik respon.
- [ ] **4.2 The Clowder Routing Matrix (`/settings/routing`):**
  - Tabel visual yang memetakan subagent ke model pilihan pengguna:
    - Meowstro (Orchestrator) ➔ Selector Model + Fallback Model.
    - Tinker (Fixer) ➔ Selector Model + Fallback Model.
    - Whisker (Explorer) ➔ Selector Model.
    - Sphinx (Oracle) ➔ Selector Model.
    - Calico (Designer) ➔ Selector Model.
  - Slider parameter granular: Temperature (0.0 - 1.0) dan Max Tokens.
- [ ] **4.3 Landlock OS Sandbox & Security Panel (`/settings/sandbox`):**
  - Status widget: `Kernel Landlock ABI v3: ACTIVE`.
  - Editor daftar direktori Read-Only dan Read-Write yang diizinkan kernel.
  - Saklar outbound HTTP/HTTPS dan visualisasi daftar subnet IP privat yang diblokir SSRF.
  - Live security audit stream viewer dengan filter semantik (`Allowed`, `Denied`, `Violation`).
- [ ] **4.4 Identity & Memory Markdown Studio (`/settings/memory`):**
  - Dual-pane Markdown visual editor untuk mengedit langsung `MEMORY.md`, `USER.md`, dan `SOUL.md`.
  - Diff visual preview saat subagent Tinker mensintesis skill prosedural baru via `manage_skill`.
- [ ] **4.5 MCP Server Hub (`/settings/mcp`):**
  - Manajemen koneksi server MCP Stdio dan SSE.
  - Live discovery viewer untuk melihat skema JSON tool yang disediakan server MCP.
- [ ] **4.6 Natural Language Cron Scheduler (`/settings/cron`):**
  - Tampilan daftar jadwal cron aktif dengan waktu eksekusi berikutnya (`next_run`).
  - Input penjadwalan berbasis bahasa alami (NL) dengan preview jadwal instan sebelum disimpan.

---

### Phase 5: Live Telemetry Inspector HUD & Mobile Ergonomics
**Tujuan:** Memberikan transparansi real-time atas apa yang sedang diproses agent dan memastikan kenyamanan akses dari perangkat mobile.

- [ ] **5.1 Right Inspector Drawer (Live Telemetry HUD):**
  - Progress bar kapasitas konteks LLM (`Context Window Usage`: e.g. `42k / 128k - 32% used`).
  - Active Subagent Hierarchy Stack (visualisasi tree pemanggilan subagent).
  - Landlock & FS I/O monitor real-time.
  - Toggle buka/tutup cepat via `Ctrl+I` / `Cmd+I`.
- [ ] **5.2 Mobile Responsive & Touch Optimization:**
  - Drawer navigasi mobile yang mulus pada layar smartphone (`<=900px`).
  - Ukuran touch target yang nyaman (minimal 44px) untuk kartu approval dan tombol aksi.
  - Input dock adaptif terhadap keyboard virtual ponsel.

---

### Phase 6: Embed, E2E Verification & LAN Dogfooding
**Tujuan:** Menyatukan seluruh hasil build ke dalam binary tunggal Rust, memvalidasi stabilitas test suite, dan melakukan uji pakai nyata di jaringan lokal.

- [ ] **6.1 Single Binary Embed Build:**
  - Hubungkan `webui/dist/` ke `crates/purrcode-channel` via `rust-embed`.
  - Verifikasi build release: `cargo build --release --bin purrcode-ai`.
  - Ukur ukuran binary akhir (harus tetap efisien di kisaran ~30–45MB, jauh di bawah standar Electron ~800MB).
- [ ] **6.2 Full Test Suite & Lint Verification:**
  - `cargo test --workspace`: Memastikan seluruh 343+ unit test backend tetap hijau 100%.
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 peringatan/error.
  - `cargo fmt --check`: Format kode Rust bersih.
- [ ] **6.3 Live LAN Dogfooding Test:**
  - Jalankan binary hasil build di PC host: `purrcode-ai web --host 0.0.0.0 --port 8080`.
  - Akses dari browser PC host (`http://127.0.0.1:8080/`) dan dari HP/laptop di jaringan LAN (`http://192.168.100.40:8080/`).
  - Uji alur lengkap: kirim prompt chat, setujui approval card diff, ubah model di Matrix tab, dan simpan pengaturan provider.

---

## 4. Matriks Risiko & Mitigasi

| Risiko | Dampak | Strategi Mitigasi |
|---|---|---|
| **Kompilasi Rust Lambat karena Aset Frontend** | Medium | Aset web hanya di-embed pada build release. Pada development, Axum mendukung opsi melayani file dari path disk lokal atau proxy ke Vite HMR (`bun dev`). |
| **Pelanggaran Aturan Ukuran File ADR-011 (<800 baris)** | Tinggi | Endpoint REST dan SSE baru dipecah modular ke beberapa file router terpisah di `crates/purrcode-channel/src/web/` (`routes_provider.rs`, `routes_sandbox.rs`, dll). |
| **Bocornya Kredensial / API Key via WebUI** | Kritis | Backend Rust tidak pernah mengirim plain text API key ke frontend. Semua API key di-mask (`sk-••••`) dan disimpan di file kredensial aman `~/.purrcode/credentials` dengan permission `0600`. |
| **Path Traversal pada Pengaturan Direktori Sandbox** | Kritis | Endpoint allowlist direktori memverifikasi `canonicalize()` dan secara ketat menolak path sensitif sistem (`/etc`, `/root`, `~/.ssh`). |
| **Regresi Perilaku Keamanan F25 (Token & Rate-Limit)** | Tinggi | Seluruh endpoint REST/SSE baru tetap berada di balik middleware auth token dan limiter laju permintaan yang sudah terverifikasi di Batch B8. |

---

## 5. Timeline & Roadmap Eksekusi

| Fase | Fokus Pekerjaan | Estimasi Langkah | Status |
|---|---|---|---|
| **Phase 0** | Workspace Svelte 5 + Bun + Vite + `rust-embed` build pipeline | 3 task | **SIAP DIEKSEKUSI** |
| **Phase 1** | Backend REST & SSE Parity Endpoints (`purrcode-service`) | 6 task | Menunggu Phase 0 |
| **Phase 2** | Tri-Pane Shell, Design System Catppuccin, Runes Store | 4 task | Menunggu Phase 1 |
| **Phase 3** | Execution Timeline, Collapsible Tool Chains, Fail-Closed Approval | 5 task | Menunggu Phase 2 |
| **Phase 4** | Management Matrices (Providers, Matrix, Landlock, Skills, MCP) | 6 task | Menunggu Phase 3 |
| **Phase 5** | Live Telemetry HUD Drawer & Mobile Touch Ergonomics | 2 task | Menunggu Phase 4 |
| **Phase 6** | Embed Release Build, E2E Verification & LAN Dogfooding | 3 task | Menunggu Phase 5 |

Rencana ini menjadi dokumen acuan utama (*single source of truth*) selama proses pembangunan WebUI generasi baru PurrCode AI.
