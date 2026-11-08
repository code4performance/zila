## ADDED Requirements

### Requirement: The postgres dialect
The `postgres` package SHALL declare the SQL dialect `postgres` with the profile: positional placeholders `$n`; single-quoted strings (no backslash escapes, `E'...'` accepted); double-quoted identifiers; `--` line comments and nesting `/* */` block comments; dollar-quoted text `$tag$...$tag$`; one statement per block. Connections returned by `postgres.open` SHALL have the dialect name `postgres`. A `sql postgres` statement SHALL run through the extended query protocol with its arguments as bound parameters.

#### Scenario: A block on a real server
- **WHEN** a `sql postgres` block with two holes is run on a PostgreSQL connection
- **THEN** the server receives the text with `$1` and `$2` and the two values as parameters, and the result is the same as the equivalent text statement

#### Scenario: A cast is not a placeholder
- **WHEN** a block contains `select {n}::int, 'a'::text`
- **THEN** `::` is ordinary text and the only hole is `{n}`

#### Scenario: pgvector
- **WHEN** a hole holds a `[]float` and the server has pgvector
- **THEN** it is sent as a vector argument, as `db.vector` does
