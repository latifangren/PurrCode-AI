# PurrCode AI — Strategi Code Intelligence & LSP (LSP.md)

Versi: 0.1 — 2026-11-11  
Status: Desain arsitektur Language Server Protocol (LSP) & Code Navigation.

---

## 1. Pelajaran Nyata dari `oh-my-pi` (`omp`)

Di `oh-my-pi`, terdapat modul `packages/coding-agent/src/lsp/lspmux.ts` yang mendukung 14 operasi LSP (`definition`, `references`, `diagnostics`, `documentSymbol`, dsb.) dengan memanggil server bahasa lokal seperti `rust-analyzer`.

Namun, menggabungkan seluruh parser AST dan debugger DAP langsung ke satu distribusi membuat ukuran binary omp membengkak hingga **316 MB**. Untuk PurrCode AI yang menargetkan single static binary musl **≤ 25 MB** di router/STB 2 GB dan HP:
- Kita **tidak** mem-bundle binary language server ke dalam PurrCode.
- PurrCode bertindak sebagai **klien LSP ringan (lightweight client)** yang berbicara dengan server bahasa yang sudah terpasang di mesin pengguna (seperti `rust-analyzer` atau `typescript-language-server`).

---

## 2. Roadmap Code Intelligence di PurrCode AI

```text
[v0.1]
  Tool Pencarian Cepat (`grep` ripgrep, `glob`, `find`, `read`)
  + CodeGraph MCP Server (`codegraph serve --mcp` — sudah ada di mcp.json user)
  + Preview Edit hash-anchored
       │
       ▼
[v0.2 / v0.3]
  Modul `purrcode-lsp` (Klien LSP JSON-RPC stdio ringan)
  - Auto-deteksi LSP lokal: rust-analyzer, tsserver, gopls, pyright, clangd
  - Tool `lsp_definition`: Lompat langsung ke file:line definisi fungsi/tipe
  - Tool `lsp_references`: Cari semua pemanggil fungsi tanpa regex palsu
  - Tool `lsp_diagnostics`: Cek error compiler/linter otomatis sebelum commit
```

---

## 3. Desain Klien LSP Ringan (`purrcode-lsp` — v0.2)

1. **Multiplexing / Deteksi Otomatis:**
   - Deteksi binary bahasa di `$PATH` berdasarkan ekstensi file proyek (`Cargo.toml` -> `rust-analyzer`, `package.json` -> `typescript-language-server`, `go.mod` -> `gopls`).
2. **Transport JSON-RPC 2.0 stdio:**
   - Memakai transport JSON-RPC yang sama persis dengan yang dipakai plugin & MCP di `purrcode-plugin`.
3. **Penyaringan Output (Token Conservation):**
   - Hasil LSP (seperti daftar 50 references) tidak ditelan mentah-mentah ke konteks LLM; diringkas menjadi `[symbol] -> file:line` yang padat (hemat token).
