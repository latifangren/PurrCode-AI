# Reverse proxy untuk WebUI (v0.4 B8, D2)

WebUI PurrCode **tidak punya TLS built-in** (keputusan D2 di `docs/v04-plan.md`:
reverse-proxy-only dulu). Untuk memakai WebUI dari HP atau dari luar localhost,
letakkan reverse proxy di depannya: TLS berakhir di proxy, token tetap diperiksa
oleh server PurrCode.

## Prasyarat

```sh
purrcode-ai web --host 127.0.0.1 --port 8080 --token <token-rahasia>
```

- `--host` default `127.0.0.1` — **jangan** bind `0.0.0.0` tanpa token dan tanpa
  proxy. Server yang bind `0.0.0.0` tanpa `--token` menerima siapa saja.
- `--token` tidak punya sumber config — hanya flag CLI. Simpan di secret store
  / environment deployment, jangan commit ke repo. Rotasi token = restart
  server dengan token baru (sesi dan history tidak hilang: keduanya di disk).
- Token dipakai sebagai `Authorization: Bearer <token>` atau `?token=<token>`
  (header + query saja — token di body tidak pernah mengautentikasi).

## Caddy (disarankan — HTTPS otomatis)

```caddyfile
purr.example.com {
    reverse_proxy 127.0.0.1:8080 {
        # SSE: teruskan event segera, jangan ditahan buffer proxy.
        flush_interval -1
    }
}
```

Caddy menerbitkan dan memperbarui sertifikat TLS sendiri (ACME). Tidak ada
pengaturan tambahan untuk SSE selain `flush_interval -1`.

## nginx

```nginx
server {
    listen 443 ssl;
    server_name purr.example.com;

    # Sertifikat dari certbot / ACME (di luar cakupan dokumen ini).
    ssl_certificate     /etc/letsencrypt/live/purr.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/purr.example.com/privkey.pem;

    # Samakan dengan batas body server (1 MiB -> 413 di atas itu).
    client_max_body_size 1m;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;

        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;

        # SSE: matikan buffering agar event mengalir langsung;
        # koneksi SSE hidup lama, jadi timeout baca harus panjang.
        proxy_buffering off;
        proxy_set_header X-Accel-Buffering no;
        proxy_read_timeout 86400s;
    }
}
```

## Yang perlu diketahui

- **SSE butuh buffering mati.** Tanpa `flush_interval -1` (Caddy) atau
  `proxy_buffering off` + `X-Accel-Buffering: no` (nginx), event (chunk jawaban,
  kartu approval, status subagent) tiba terlambat atau bergerombol.
- **Rate limit prompt efektif global di balik proxy satu host.** Server
  membatasi `POST /api/prompt` per peer IP (30/menit, B8 D9), dan semua request
  dari proxy tiba dari `127.0.0.1` — jadi semua pengguna proxy berbagi satu
  ember. Untuk pemakaian satu user ini sudah cukup; bila nanti ada banyak
  pengguna, batasi per-IP di proxy itu sendiri.
- **Limit auth gagal bersifat global** (10/menit, B8 D9) — sengaja, karena
  per-IP runtuh di balik proxy satu host. Hanya percobaan gagal yang dibatasi;
  token benar selalu lolos, jadi pemilik tidak pernah terkunci oleh percobaan
  orang lain.
- **Health check proxy:** `GET /api/status` (butuh token bila dikonfigurasi).
- **Sesi idle 30 menit** (B8 D8): entri live tanpa subscriber SSE dan tanpa
  turn berjalan diusir dari memori; history tetap di disk dan sesi terwujud
  lagi saat dibuka — tab browser yang terbuka tidak kehilangan chat.
- **Audit:** semua aksi web (POST), percobaan auth gagal (401), dan penolakan
  rate limit (429) tercatat di `<purrcode_dir>/audit.jsonl` dengan
  `session_id`/`agent` = `web`. GET (baca) tidak diaudit.
