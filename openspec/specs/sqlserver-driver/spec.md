# sqlserver-driver Specification

## Purpose
Connects Brasa programs to Microsoft SQL Server with a driver written in Brasa that implements the same database interface as the PostgreSQL driver.

## Requirements

### Requirement: Opening a connection
`sqlserver.open(url)` SHALL parse `sqlserver://user:password@host:port/database?encrypt=strict` (port default 1433; named instances are not supported) and return `db.Conn or error`. The accepted `encrypt` values SHALL be `strict` (TDS 8.0: TLS first, with the ALPN protocol `tds/8.0`, certificate and host name validated; `cafile=<path>` adds the CA certificates in that PEM file as trusted for this connection only, and there is no option that turns validation off), `off` (no encryption at all: the password is only obfuscated, as TDS defines it, so this is for local development; a server that requires encryption makes `open` fail with a message that suggests `strict`) and, for the default, `strict`. The value `on` (TLS inside the TDS pre-login exchange) SHALL be rejected with an error that says it is not supported and suggests `strict` or `off`. Windows integrated authentication SHALL NOT be supported; only SQL login. A wrong login SHALL give an error of kind `Unauthenticated`.

#### Scenario: Strict encryption
- **WHEN** a program opens a `sqlserver://` URL with `encrypt=strict` against SQL Server 2022 or later and runs `select 1`
- **THEN** the query returns the value 1

#### Scenario: Unsupported mode
- **WHEN** the URL has `encrypt=on`
- **THEN** `open` returns an error before any connection attempt

#### Scenario: Plain, local development
- **WHEN** the URL has `encrypt=off` and the server does not force encryption
- **THEN** the connection opens and queries work

### Requirement: Named instances and named pipes
The URL host MAY be `host\INSTANCE`. For an instance on this machine (`.`, `localhost`, `(local)`) with no port, the driver SHALL connect through the instance's named pipe (`\\.\pipe\MSSQL$INSTANCE\sql\query`) on Windows, with `encrypt=off` as the default because a pipe is local; `?transport=pipe` (optionally with `?pipe=<path>`) and `?transport=tcp` SHALL choose the transport explicitly. For any other host the driver SHALL ask the SQL Server Browser (UDP 1434) for the instance's TCP port and connect to it, and SHALL fail with a message that says so when the Browser does not answer or the instance does not listen on TCP. `encrypt=strict` over a named pipe SHALL be refused, since a pipe cannot carry the TLS of TDS 8.0. On Linux a named pipe SHALL give an error of kind `Unsupported`.

#### Scenario: A local named instance
- **WHEN** a program opens `sqlserver://ai:ai@.\SQL2025/njai` on a machine whose instance SQL2025 has TCP disabled and runs `select 1`
- **THEN** the query returns 1 through the named pipe

#### Scenario: Strict over a pipe
- **WHEN** the URL names an instance on this machine and asks for `encrypt=strict`
- **THEN** `open` fails with an error of kind `InvalidInput` before sending anything

### Requirement: Parameterized execution
Statements with arguments SHALL run as a remote procedure call (`sp_executesql`) with typed parameters named `@p1`, `@p2` and so on, never with values spliced into the text. Statements without arguments SHALL run as a SQL batch. Result rows SHALL be read completely, with the same `max_rows` limit as the PostgreSQL driver. The data types SHALL cover integers (`tinyint` to `bigint`), `bit`, `float`/`real`, `decimal`/`numeric`/`money` (as text), `char`/`varchar` (single-byte data in code page 1252, 1250 or 1251 or in a UTF-8 collation; another code page is an error), `nchar`/`nvarchar` (UTF-16 converted to and from UTF-8), `date`/`time`/`datetime2`/`datetimeoffset` (ISO 8601 text), `uniqueidentifier` (text), `xml` (text), `varbinary` and `NULL`. A column of an unsupported type SHALL be an error that names the column and the type, not garbage data.

#### Scenario: Parameters and Unicode
- **WHEN** a program inserts the name `Zoë 日本` with `conn.exec("insert into people(name) values(@p1)", name)` and reads it back
- **THEN** the stored text is identical

#### Scenario: Unsupported type
- **WHEN** a query selects an `xml` column
- **THEN** the call returns an error naming the column and the type `xml`

### Requirement: Same interface as other drivers
The connection SHALL implement `db.Conn` with the semantics of `use db`: typed rows, `begin`/`commit`/`rollback` (as `BEGIN TRANSACTION` with the transaction descriptor of the protocol), error kinds (error number 2627 and 2601 give `AlreadyExists`, 18456 gives `Unauthenticated`, 229 and 297 `PermissionDenied`) and cooperative waiting. A server error SHALL include its number, severity and state in the message.

#### Scenario: Unique violation
- **WHEN** a program inserts a duplicate primary key
- **THEN** the error has kind `AlreadyExists` and the message contains `2627`

#### Scenario: Transaction rollback
- **WHEN** a program inserts a row in a transaction and rolls it back
- **THEN** a later `select count(*)` does not count the row

### Requirement: Protocol robustness
The driver SHALL handle packets split across several network reads and results spanning several TDS packets (packet size negotiated, default 4096), SHALL answer a server `ATTENTION`/cancel correctly when a statement times out, and SHALL return an error, not hang or crash, on a truncated or malformed token stream.

#### Scenario: Large result
- **WHEN** a query returns 100,000 rows of 200 bytes
- **THEN** all rows are returned correctly

#### Scenario: Garbage from server
- **WHEN** a fake server sends a token stream cut in the middle of a row
- **THEN** the call returns an error and the connection is closed
