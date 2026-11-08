## Why

`examples/rest_api.br` already shows `db.open("postgres://...")` and `conn.query(Customer, "select ...")`, and `db` does not exist: Zila Lang programs cannot talk to a database at all. Databases are also the base of the AI work that follows (`add-ai-client`, `add-mcp`, `add-rag`: tools that query data, retrieval over pgvector). This change adds the missing foundations, written in Zila Lang over `net` and `tls`: a driver-neutral `db` library, PostgreSQL and SQL Server drivers, the `crypto` primitives their logins need, the two small `tls` additions they depend on, and a Docker environment with real PostgreSQL 19 and SQL Server 2025 servers to test against.

## What Changes

- **`use crypto`**: SHA-256, HMAC-SHA-256, PBKDF2-HMAC-SHA-256, MD5, base64/base64url, hex, constant-time `equal` and `random` from the operating system.
- **`use db`**: the `db.Conn` interface with parameterized `exec`, `query(T, sql, args...)` and `one(T, sql, args...)` that decode rows into the user's records by comptime reflection, transactions, error kinds, password redaction.
- **`use postgres`**: PostgreSQL wire protocol v3 over `net`/`tls`, extended query protocol, SCRAM-SHA-256 (and md5), `sslmode`, `sslrootcert`, pgvector values.
- **`use sqlserver`**: TDS 7.4 over `net`/`tls`, SQL login, `sp_executesql` parameters, `encrypt=strict` (TDS 8.0) and `encrypt=off`, `cafile`.
- **`use tls`** gains `tls.upgrade(conn, host)` (start TLS on an open connection, for PostgreSQL), `tls.connect_with` with ALPN (for TDS 8.0) and `ca_file` for private certificate authorities (needed to test against a development CA without touching the system trust store).
- **Docker test environment** in `tests/docker/`: PostgreSQL 19 with pgvector (SCRAM and TLS) and SQL Server 2025 (plain and strict ports), the same seeded `shop` database in both, a development CA, scripts to set the `ZILA_TEST_*` variables. Real-server tests run only when those variables are set.
- **Example** `examples/db.br` (the same function on both databases); `examples/rest_api.br` and `examples/hello.br` stop referring to the non-existent `db.postgres`.
- **Documentation**: a new `docs/databases.html` page plus stdlib, native and navigation updates.

Non-goals: MySQL, SQLite, Oracle drivers; an ORM or query builder; connection pooling beyond what the interface allows a user to write; Windows integrated authentication; SQL Server named instances; TDS `encrypt=on` (TLS inside the TDS pre-login; needs a transport hook in `tls`, a follow-up); binary result format for PostgreSQL; `COPY`, `LISTEN/NOTIFY`, cursors.

## Capabilities

### New Capabilities

- `crypto-library`: hashes, HMAC, PBKDF2, base64, hex and secure random bytes.
- `db-library`: driver-neutral database interface, typed row decoding, parameters, transactions and errors.
- `postgres-driver`: PostgreSQL over the wire protocol, TLS and pgvector.
- `sqlserver-driver`: SQL Server over TDS, strict encryption and plain.
- `test-databases`: the Docker environment, seed data and test selection.

### Modified Capabilities

- `tls-client`: upgrading an open connection, ALPN and a private CA file.

## Impact

New `std/crypto.br`, `std/db.br`, `std/postgres.br`, `std/sqlserver.br`; `std/tls.br` and `std/tls.linux.br`; `src/codegen_x64.rs` (library tables, kernel rejection); `tests/docker/` (new: `compose.yml`, optional `Dockerfile` for PostgreSQL, init SQL, CA generation script, `env.ps1`, `env.sh`, `README.md`), `tests/crypto.rs`, `tests/db.rs`, `tests/tls.rs`, `tests/linux.rs`, `tests/data/` (recorded exchanges), `.gitignore` (certs), `examples/db.br`, `examples/rest_api.br`, `examples/hello.br`, `docs/databases.html`, `docs/stdlib.html`, `docs/native.html`, docs navigation, `README.md`. No dependency on other pending changes: this is the first of four (`add-ai-client`, `add-mcp` and `add-rag` follow, and `add-rag` uses the pgvector store from here). Depends on the archived `add-json`, `add-comptime-reflection`, `add-error-kind`, `remove-async-coloring` and `add-tls`.
