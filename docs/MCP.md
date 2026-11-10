# PurrCode AI — Integrasi MCP & Ekosistem Tool (MCP.md)

Versi: 0.1 — 2026-11-11  
Status: Desain integrasi Model Context Protocol (MCP) sebagai jembatan tool eksternal.  
Kompatibilitas: **100% kompatibel dengan Claude Code & oh-my-pi (`mcp.json`)**.

---

## 1. Mengapa MCP Sangat Penting di PurrCode AI

Bagi *builder* dan *vibe coder*, membuat tool baru secara native di Rust (seperti browser automation, GitHub API, database viewer, crawler) membutuhkan waktu lama dan membuat binary bengkak.

Dengan mendukung MCP:
- Pengguna tinggal menghubungkan server MCP yang sudah ada (Playwright, Crawl4AI, CodeGraph, Context7, Chrome DevTools, GitHub, dll.).
- PurrCode AI membaca konfigurasi `~/.purrcode/mcp.json` (atau `.purrcode/mcp.json` per proyek).
- Tool-tool dari server MCP otomatis terdaftar di `ToolRegistry` dan bisa langsung dipakai oleh subagent (Tinker, Whisker, Sphinx).

---

## 2. Format Konfigurasi `mcp.json`

PurrCode AI mengadopsi format kanonik yang sama persis dengan `oh-my-pi` dan `Claude Code`:

```jsonc
// ~/.purrcode/mcp.json, .purrcode/mcp.json (mendukung key "mcpServers" maupun "mcp_servers"),
// atau langsung di blok "mcp_servers" di dalam ~/.purrcode/purrcode.jsonc
{
  "$schema": "https://raw.githubusercontent.com/can1357/oh-my-pi/main/packages/coding-agent/src/config/mcp-schema.json",
  "mcpServers": {
    // 1. Transport stdio (menjalankan proses lokal)
    "playwright": {
      "command": "cmd.exe",
      "args": ["/c", "npx", "-y", "@playwright/mcp@latest"],
      "env": {
        "DEBUG": "pw:api"
      }
    },
    "codegraph": {
      "command": "codegraph",
      "args": ["serve", "--mcp"]
    },
    // 2. Transport HTTP / SSE (server remote)
    "crawl4ai": {
      "type": "http",
      "url": "https://api.crawl4ai.com/mcp",
      "headers": {
        "Authorization": "Bearer ${CRAWL4AI_API_KEY}"
      }
    },
    "context7": {
      "type": "http",
      "url": "https://mcp.context7.com/mcp"
    }
  }
}
```

---

## 3. Arsitektur Klien MCP di Rust (`crates/purrcode-tools/src/mcp/`)

```text
               ┌───────────────────────────────────────────────┐
               │           PurrCode AI (Rust Core)             │
               │                                               │
               │  ┌─────────────────┐     ┌──────────────────┐ │
               │  │   mcp.json      │────►│ McpClientManager │ │
               │  └─────────────────┘     └────────┬─────────┘ │
               │                                   │ spawns    │
               │                                   ▼           │
               │                          ┌──────────────────┐ │
               │                          │   ToolRegistry   │ │
               │                          │ (dynamic inject) │ │
               │                          └──────────────────┘ │
               └───────────────────────────────────┬───────────┘
                                                   │ stdio / SSE (JSON-RPC 2.0)
                        ┌──────────────────────────┴──────────────────────────┐
                        ▼                                                     ▼
         ┌─────────────────────────────┐                       ┌─────────────────────────────┐
         │ Node.js / Python MCP Server │                       │    Remote HTTP/SSE Server   │
         │ (Playwright, DevTools, dll) │                       │     (Context7, Crawl4AI)    │
         └─────────────────────────────┘                       └─────────────────────────────┘
```

### 3.1 Siklus Hidup (Lifecycle)
1. **Startup Discovery:** Saat engine boot, `McpClientManager` membaca `~/.purrcode/mcp.json` dan `.purrcode/mcp.json`.
2. **Koneksi & Handshake:**
   - Transport `stdio`: Spawn child process (`tokio::process::Command`), sambungkan `stdin`/`stdout`.
   - Kirim request `initialize` JSON-RPC 2.0, tunggu respon capabilities.
3. **Pendaftaran Tool:**
   - Kirim `tools/list`.
   - Setiap tool yang dilaporkan dibungkus menjadi implementasi trait `Tool` Rust dengan prefix `mcp__<server_name>__<tool_name>` (misal `mcp__playwright__browser_click`).
   - Didaftarkan ke `ToolRegistry`.
4. **Eksekusi:** Saat model memanggil tool, wrapper mengirim `tools/call` via JSON-RPC dan mengembalikan hasilnya ke loop agent.
5. **Shutdown:** Saat sesi selesai atau runtime di-interrupt, seluruh child process MCP di-kill secara graceful.

---

## 4. Keamanan & Sanitasi Secret
- **Environment Scrubbing:** Sebelum men-spawn server stdio, variabel env yang memuat API key LLM PurrCode (`PURRCODE_API_KEY`, credential provider) disaring/dibersihkan dari child process kecuali yang eksplisit dideklarasikan di blok `"env"`.
- **Approval Tier:** Seluruh tool MCP secara default masuk ke tier **`ApprovalTier::Exec`** (atau `ApprovalTier::Write`), sehingga di mode `supervised` (Tabby) tetap meminta konfirmasi pengguna sebelum aksi destruktif dijalankan.
