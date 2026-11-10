# PurrCode AI — Protokol Subagent & Handoff (SUBAGENTS.md)

Versi: 0.1 — 2026-11-11  
Status: Spesifikasi teknis mekanisme anti-hilang-konteks The Clowder.

---

## 1. Masalah yang Diselesaikan

Pada sebagian besar multi-agent harness, orchestrator mendelegasikan tugas ke subagent hanya melalui prompt teks (*"kamu adalah agen X, tolong kerjakan Y"*). Saat subagent selesai, parent sering menelan seluruh percakapan subagent (konteks jebol), atau bila turn terputus, parent men-spawn ulang subagent baru dari nol (*respawn loop* yang boros token dan kehilangan riwayat penyelidikan).

PurrCode AI menerapkan 4 pilar teknis:
1. **Continuation ID (`purr_<id>`):** ID deterministik stabil untuk setiap anak. Melanjutkan tugas = memanggil ID yang sama.
2. **File Handoff Fisik:** Tugas dicatat di `brief.md`, hasil dicatat di `result.md`. Tidak hilang meskipun proses mati.
3. **Registry Sesi:** Metadata tersimpan di `subagents/<id>.json`.
4. **Ringkasan Terstruktur (Bukan Dump Raw):** Anak hanya mengembalikan ringkasan padat ke parent.

---

## 2. Struktur Direktori Delegasi

```text
~/.purrcode/sessions/<session_id>/
├── subagents/
│   ├── purr_a1b2c3.json         # Metadata registry
│   └── purr_a1b2c3/
│       ├── brief.md             # Instruksi dari Meowstro
│       └── result.md            # Laporan hasil dari subagent
```

### 2.1 Format `brief.md` (Ditulis oleh Meowstro saat `task()` dipanggil)
```markdown
# Brief Delegasi: [Judul Tugas Singkat]
- **Subagent ID:** purr_a1b2c3
- **Role:** tinker (Fixer)
- **Target File / Area:** src/auth.rs:40-60
- **Objektif:** Perbaiki bug login crash saat password bernilai string kosong.
- **Batasan:** Jangan ubah signature fungsi `verify_password`; tambahkan unit test.
- **Expected Output:** Patch minimal, unit test verifikasi, daftar file yang diubah.
```

### 2.2 Format `result.md` (Ditulis oleh Subagent sebelum selesai)
```markdown
# Laporan Hasil: purr_a1b2c3
- **Status:** completed
- **Changed Files:**
  - `src/auth.rs:42` (tambahkan pengecekan empty password)
  - `tests/auth_tests.rs:18` (unit test empty password)
- **Verified Evidence:**
  - `cargo test --test auth_tests` -> 4 passed, 0 failed.
- **Failures / Blockers:**
  - Tidak ada.
- **Unverified Items:**
  - Integrasi LDAP belum diuji karena server staging tidak aktif.
```

### 2.3 Format `subagents/<id>.json` (State Registry)
```json
{
  "id": "purr_a1b2c3",
  "parent_id": "meowstro_root",
  "role": "tinker",
  "status": "done",
  "created_at": "2026-11-11T02:00:00Z",
  "updated_at": "2026-11-11T02:03:15Z",
  "tokens_used": {
    "input": 14200,
    "output": 1850
  },
  "summary": "Memperbaiki crash empty password di src/auth.rs:42 dan menambahkan unit test lulus."
}
```

---

## 3. Aturan Enforce Runtime (Bukan Sekadar Prompt)

1. **Meowstro Read-Only:** Di `crates/purrcode-orchestrator`, preset tool Meowstro **tidak memiliki** `write`, `edit`, dan `bash`. Bila model Meowstro halusinasi memanggil `edit`, harness langsung menolak dengan error `PurrError::ToolNotInPreset`.
2. **Anti-Respawn:** Pemanggilan `task(role="tinker", id="purr_a1b2c3", ...)` otomatis melanjutkan konteks child yang sudah ada di registry.
3. **Max Depth = 1 di v0.1:** Subagent tidak diizinkan memanggil `task()` lagi (mencegah ledakan rekursi subagent tak terkendali).
