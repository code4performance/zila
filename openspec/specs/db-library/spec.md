# db-library Specification

## Purpose
A single way to talk to a relational database from Zila Lang: parameterized statements, rows decoded into the program's own types, transactions and clear errors, independent of which database sits behind it.

## Requirements

### Requirement: The Conn type
`use db` SHALL define the type `db.Conn`, a connection that every driver (`postgres.open(url)`, `sqlserver.open(url)`) returns, so code written against `db.Conn` runs unchanged on every driver. It SHALL offer the methods `conn.exec(sql, args...) -> int or error` (rows affected), `conn.begin() -> Tx or error` and `conn.close()`, and the functions `db.query(T, conn, sql, args...) -> []T or error` and `db.one(T, conn, sql, args...) -> T or error` (free functions, because the type argument is read at build time). `db.Tx` SHALL carry a `conn` to run statements in the transaction and offer `commit()` and `rollback()`. `one` SHALL return an error of kind `NotFound` for zero rows and `InvalidInput` for more than one. A connection that goes out of scope SHALL be closed, and a transaction that goes out of scope without `commit` or `rollback` SHALL be rolled back before the connection's next statement.

#### Scenario: Same code, two drivers
- **WHEN** a function `top_customers(conn: db.Conn) -> []Customer or error` that calls `db.query` is called once with a PostgreSQL connection and once with a SQL Server connection over the same data
- **THEN** both calls return the same list

#### Scenario: One row expected
- **WHEN** `db.one(Customer, conn, "select * from customers where id = $1", db.int(999))` matches no row
- **THEN** the result is an error of kind `NotFound`

### Requirement: Parameters are always parameters
Values passed after the SQL text SHALL be sent to the server as bound parameters, never spliced into the SQL text by the library. They SHALL be values of `db.Arg`, made with `db.int(n)`, `db.float(x)`, `db.bool(b)`, `db.str(s)`, `db.bytes(data)`, `db.vector(xs)` (a `[]float`, for drivers that support vectors) and `db.null()` (SQL `NULL`). The placeholder syntax of the database SHALL be used as written (`$1` for PostgreSQL, `@p1` for SQL Server). A value that is not a `db.Arg` (a bare `5` or `"x"`) SHALL be a compile error naming its type.

#### Scenario: Injection text is data
- **WHEN** a program inserts the name `x'); drop table customers; --` through `conn.exec("insert into customers(name) values($1)", db.str(name))` and reads it back
- **THEN** the stored name is exactly that text and the table still exists

#### Scenario: Null
- **WHEN** a program passes `db.null()`
- **THEN** the row stores `NULL`, and reading it back into a `text?` field gives `nil`

### Requirement: Typed rows
`db.query(T, conn, ...)` and `db.one(T, conn, ...)` SHALL build values of `T` where `T` is a record, matching columns to fields by name (case-insensitively, with `#[db("column_name")]` to rename and `#[db(skip)]` to leave a field out), or a single scalar type for a one-column result. The decoding SHALL be derived by comptime reflection, without registering types. A column with no field SHALL be ignored; a field with no column, and a `NULL` in a field that is not optional, SHALL be errors that name the field and the column; a value that does not fit the field's type (a text in an `int`, an integer out of range) SHALL be an error that names the field and the row number. Numbers, booleans, text, timestamps (as text in ISO 8601), UUIDs (as text) and decimals (as text, never rounded through a float unless the field is a float) SHALL be supported.

#### Scenario: Record by column name
- **WHEN** `select id, name from customers` is read as `[]Customer` with fields `id: int` and `name: text`
- **THEN** each row becomes a `Customer`

#### Scenario: Missing and null
- **WHEN** a query returns a `NULL` name for a field `name: text`, and another query lacks a column named `age` for a field `age: int`
- **THEN** each returns an error naming the field and the column

#### Scenario: Rename
- **WHEN** a field `#[db("full_name")] name: text` reads `select full_name from people`
- **THEN** the field holds the column value

### Requirement: Transactions
`conn.begin()` SHALL start a transaction whose statements run through `tx.conn`. `tx.commit()` SHALL make its changes visible and end it; `tx.rollback()` SHALL undo them. A transaction that is dropped without `commit` SHALL roll back before the connection's next statement (waiting for the network is not possible while a value is being dropped). A statement error inside a transaction SHALL be returned and SHALL leave the transaction usable only for `rollback` on databases that abort it. Using the connection itself while a transaction on it is open, beginning a second transaction, and using a transaction after it ended SHALL be errors.

#### Scenario: Rollback on drop
- **WHEN** a function inserts a row inside `begin()`, returns an error before `commit`, and the caller then counts rows
- **THEN** the row is not there

#### Scenario: One user of the connection at a time
- **WHEN** a transaction is open and the program calls `conn.exec(...)` or `conn.begin()` on the connection itself
- **THEN** each returns an error that tells it to use the transaction's `conn` or end it

### Requirement: Errors
Database errors SHALL be values of the standard error type with a kind from `use errors`: `Unavailable` for connection failures, `Unauthenticated` for a rejected login, `PermissionDenied`, `NotFound`, `AlreadyExists` for a unique-constraint violation, `InvalidInput` for syntax errors and constraint violations, and `Timeout`. The message SHALL include the server's text and, when the server gives it, its code (the SQLSTATE for PostgreSQL, the error number for SQL Server). The connection URL's password SHALL NOT appear in any message, including the messages for a malformed URL. The library SHALL provide `db.parse_url` for the drivers (scheme, user, password, host, port, database and query parameters, percent-decoded) and `db.redact(url)` (the URL with the password replaced by `***`).

#### Scenario: Unique violation
- **WHEN** a program inserts a second row with the same primary key
- **THEN** the error has kind `AlreadyExists`

#### Scenario: Password never printed
- **WHEN** a connection to `postgres://app:s3cret@localhost/shop` fails
- **THEN** the error message does not contain `s3cret`

### Requirement: Cooperative waiting
Every call SHALL wait without blocking other tasks: while one task waits for the database, others run.

#### Scenario: Slow query does not stall the program
- **WHEN** one task runs a query that takes two seconds and another task prints a line every 100 ms
- **THEN** the lines keep appearing during the query

### Requirement: The Sql type
`use db` SHALL define `db.Sql`, a record holding a dialect name (`text`), the final statement text (`text`) and its arguments (`[]Arg`), the value of a `sql` block. It SHALL offer `sql.text`, `sql.dialect` and `sql.args` as read-only fields. A `db.Conn` SHALL have a `dialect` field with the name its driver sets (`"postgres"`, `"tsql"`), empty for a connection made with `db.connection` without one; a connection with an empty dialect SHALL accept a `db.Sql` of any dialect.

#### Scenario: Fields of a statement
- **WHEN** a `sql postgres` block with one hole is evaluated
- **THEN** `q.dialect` is `postgres`, `q.text` has the placeholder `$1` and `q.args` has one element

#### Scenario: A hand-made connection
- **WHEN** a test double built with `db.connection` and no dialect runs a `sql tsql` statement
- **THEN** it is accepted

### Requirement: Running a Sql value
`conn.rows_sql(q)`, `conn.exec_sql(q)`, `db.query_sql(T, conn, q)` and `db.one_sql(T, conn, q)` SHALL behave as `conn.rows`, `conn.exec`, `db.query` and `db.one` with `q.text` and `q.args`, including typed rows, transactions, error kinds and the rule that values travel as bound parameters. They SHALL return an error of kind `InvalidInput` when `q.dialect` and a non-empty `conn.dialect` differ.

#### Scenario: Typed rows from a block
- **WHEN** `db.query_sql(Customer, conn, q)` runs a block that selects `id` and `name`
- **THEN** the result is the list of `Customer` values, as `db.query` would give for the same text and arguments

#### Scenario: Inside a transaction
- **WHEN** `tx.conn.exec_sql(q)` runs an insert and the transaction is rolled back
- **THEN** the row is not stored
