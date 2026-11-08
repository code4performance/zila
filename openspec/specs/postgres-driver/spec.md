# postgres-driver Specification

## Purpose
Connects Brasa programs to PostgreSQL with a driver written in Brasa, including encrypted connections, modern authentication and the vector type used for embeddings.

## Requirements

### Requirement: Opening a connection
`postgres.open(url)` SHALL parse a URL of the form `postgres://user:password@host:port/database?sslmode=require` (`postgresql://` too; port default 5432; `user`, `password` and `database` percent-decoded) and return `db.Conn or error`. `sslmode=disable` SHALL use plain TCP, `sslmode=require` SHALL send the `SSLRequest`, upgrade with `tls.upgrade` and validate the server certificate and host name (`sslrootcert=<path>` adds the CA certificates in that PEM file as trusted for this connection only, for servers with a private CA; it never turns validation off), and the default (`prefer` is not supported) SHALL be `require`. If the server refuses encryption under `require`, the open SHALL fail. A malformed URL SHALL be an `InvalidInput` error.

#### Scenario: Local server without TLS
- **WHEN** a program opens `postgres://app:pw@localhost/shop?sslmode=disable` against a running server and runs `select 1`
- **THEN** the query returns the value 1

#### Scenario: Require refuses plain server
- **WHEN** the URL uses `sslmode=require` and the server answers `N` to the `SSLRequest`
- **THEN** `open` returns an error and no password is sent

### Requirement: Authentication
The driver SHALL support SCRAM-SHA-256 (verifying the server signature), `md5` and cleartext password requests (cleartext only on an encrypted connection), and SHALL reject any other method with an error that names it. A wrong password SHALL give an error of kind `Unauthenticated`.

#### Scenario: SCRAM
- **WHEN** a server configured for `scram-sha-256` accepts the correct password
- **THEN** the connection opens, and with a wrong password `open` returns `Unauthenticated`

#### Scenario: Forged server signature
- **WHEN** a fake server completes SCRAM with an incorrect server signature
- **THEN** `open` returns an error and the connection is closed

### Requirement: Extended query protocol
Statements SHALL be run with the extended query protocol (Parse, Bind, Describe, Execute, Sync) so parameters are bound, not interpolated. Several statements in one `exec` text SHALL NOT be accepted when they have parameters. Result values SHALL be read in the text format. Results SHALL be fully read before the call returns, with a limit `max_rows` (default 1,000,000) that gives an error instead of exhausting memory.

#### Scenario: Parameters
- **WHEN** a program runs `select $1::int + $2::int` with 2 and 3
- **THEN** it reads 5

#### Scenario: Row limit
- **WHEN** a query returns more rows than `max_rows`
- **THEN** the call returns an error saying so and the connection stays usable

### Requirement: pgvector
Arguments of type `[]float` SHALL be sent as a `vector` value and a `vector` column SHALL be readable into a `[]float` field, so embeddings can be stored and searched with the operators of the `pgvector` extension (`<=>`, `<->`, `<#>`). If the extension is missing the server's error SHALL be returned unchanged.

#### Scenario: Nearest neighbors
- **WHEN** a program inserts three rows with 3-dimensional vectors and runs `select id from items order by embedding <=> $1 limit 1` with a query vector close to the second row
- **THEN** it reads the second row's id

### Requirement: Server notices and lifecycle
`NOTICE` messages SHALL be ignored unless `postgres.on_notice` is set. `close()` SHALL send `Terminate` and close the socket. A connection that the server closes (for example on restart) SHALL make the next call return an `Unavailable` error and SHALL NOT be reused.

#### Scenario: Server restarts
- **WHEN** the server closes the connection between two queries
- **THEN** the second query returns an `Unavailable` error
