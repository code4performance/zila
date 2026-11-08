## Purpose

Gives contributors one command to start real PostgreSQL and SQL Server servers with known data, so that the database drivers are checked against the real protocols and not only against fakes.

## ADDED Requirements

### Requirement: Local database environment
The repository SHALL contain a Docker Compose environment under `tests/docker/` that starts a PostgreSQL 19 server with the `pgvector` extension available and a SQL Server 2025 (Developer edition) server, with `docker compose -f tests/docker/compose.yml up -d --wait` returning only when both accept connections. The environment SHALL also run two plain TLS servers for the `tls` tests (one that ignores ALPN and one that selects `http/1.1`), using the same certificate. Ports SHALL be published on `127.0.0.1` only, on ports that do not collide with default installations (PostgreSQL 55432, SQL Server 51433 and 51434, TLS test servers 55443 and 55444). Credentials SHALL be development-only values defined in one file under `tests/docker/`, never reused outside the environment. Data SHALL live in named volumes so that `docker compose ... down -v` removes everything. The environment SHALL NOT be started by `cargo test` or by any build.

#### Scenario: Up and healthy
- **WHEN** a contributor runs the `up` command on a machine with Docker
- **THEN** it returns with both services healthy and the published ports answer only on the loopback address

#### Scenario: Clean removal
- **WHEN** the contributor runs `down -v`
- **THEN** no containers or volumes of the environment remain

### Requirement: Seed data and encryption setups
Both servers SHALL start with a database `shop` holding the tables `customers` and `orders` with the same rows on both servers (at least 50 customers, including names with accents, an apostrophe and a non-Latin script, and rows with `NULL` columns), PostgreSQL SHALL also hold a table `items` with a `vector(3)` column and sample rows, and each server SHALL have a read-write user and a read-only user. PostgreSQL SHALL require SCRAM-SHA-256 and TLS with a certificate signed by a development CA that the environment generates; SQL Server SHALL accept `encrypt=off` on one port and, on another, force encryption with a certificate signed by the same CA, accept TDS 8.0 strict clients (ALPN `tds/8.0`) and refuse clients that do not use TLS. The CA certificate SHALL be written to `tests/docker/certs/ca.pem` (ignored by git) for use as `sslrootcert`/`cafile`.

#### Scenario: Same data
- **WHEN** `select count(*) from customers` runs on both servers
- **THEN** both return the same number

#### Scenario: Strict only
- **WHEN** a client connects to the strict SQL Server port without TLS
- **THEN** the connection cannot be established

### Requirement: Test selection through environment variables
`tests/db.rs` and `tests/rag.rs` SHALL run their real-server tests only when `BRASA_TEST_POSTGRES` (a URL), `BRASA_TEST_SQLSERVER` (a URL for the plain port) or `BRASA_TEST_SQLSERVER_STRICT` (a URL for the strict port) are set, and SHALL print a line saying which test was skipped and which variable enables it otherwise. A script `tests/docker/env.ps1` and `tests/docker/env.sh` SHALL set these variables for the environment above. A skipped test SHALL NOT count as a failure.

#### Scenario: Skipped without variables
- **WHEN** `cargo test --test db` runs with no `BRASA_TEST_*` variable
- **THEN** the real-server tests print a skip line each and the run passes

#### Scenario: Run with the environment
- **WHEN** the contributor starts the environment, sources `env.ps1` and runs `cargo test --test db`
- **THEN** the real-server tests run against both servers and pass
