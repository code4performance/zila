## ADDED Requirements

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
