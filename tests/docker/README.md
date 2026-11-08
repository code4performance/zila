# Local test databases

Real PostgreSQL 19 (with pgvector) and SQL Server 2025 servers for the `postgres` and `sqlserver`
drivers. Optional: nothing in `cargo build` or `cargo test` starts them, and tests that need them
print a skip line when the variables below are not set.

```
docker compose -f tests/docker/compose.yml up -d --wait     # first run builds the image and pulls ~1.5 GB
. tests/docker/env.ps1                                       # PowerShell (or: source tests/docker/env.sh)
cargo test --test db
docker compose -f tests/docker/compose.yml down -v           # removes containers and data
```

| Service | Port (127.0.0.1) | Notes |
|---|---|---|
| `postgres` | 55432 | `postgres:19rc1` + pgvector 0.8.7 built from source; `hostssl` + `scram-sha-256` only; users `postgres`, `app` (read-write), `reader` (read-only) |
| `sqlserver` | 51433 | encryption not forced (`encrypt=off` works); login `sa`, `app` (db_owner), `reader` (db_datareader) |
| `sqlserver-strict` | 51434 | encryption forced with a certificate from the development CA; negotiates ALPN `tds/8.0` (TLS 1.3); a client without TLS cannot connect |

| `tls-plain` / `tls-alpn` | 55443 / 55444 | `openssl s_server` with the same certificate, for the `tls` tests: the first ignores ALPN, the second selects `http/1.1` |

Both databases hold `shop` with `customers` (60 rows: accents, an apostrophe, CJK and Cyrillic names,
`NULL` emails and cities) and `orders` (180 rows). PostgreSQL also has `items(id, label, embedding vector(3))`.

Variables set by `env.ps1` / `env.sh`: `BRASA_TEST_POSTGRES`, `BRASA_TEST_SQLSERVER`,
`BRASA_TEST_SQLSERVER_STRICT`. The development CA is created on first `up` in `certs/` (git-ignored);
`certs/ca.pem` is what the URLs pass as `sslrootcert` / `cafile`.

Credentials are development-only values in `.env` and the seed files; the ports are bound to the
loopback address. Do not reuse any of them elsewhere.

Recording protocol exchanges for the replay tests (`tests/data/`) is described next to those tests.
