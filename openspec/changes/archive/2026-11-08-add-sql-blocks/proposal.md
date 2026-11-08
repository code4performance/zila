## Why

SQL in Brasa today is a text string passed to `conn.exec(sql, db.int(n), ...)`. The text is not checked until the server rejects it, the placeholders are the database's own (`$1` on PostgreSQL, `@p1` on SQL Server) so the same query cannot be written once, and every value needs a hand-written `db.int(...)`/`db.str(...)` wrapper that is easy to get out of step with the placeholder number. The language already has a precedent for another language written inline and kept raw: the `asm` block. This change gives SQL the same treatment: lines written as they are, a dialect that says how to read them, and values that can only ever travel as bound parameters.

## What Changes

- **A `sql <dialect>` block**, written like `asm`: raw lines until a line that is only `end`. It is an expression whose value is a `db.Sql` (dialect name, final text, bound arguments).

  ```brasa
  let q = sql postgres
      select id, name from customers
      where id < {limit} and city = {city}
  end
  let rows = try db.query_sql(Customer, conn, q)
  ```

  `sql tsql` is the SQL Server dialect. The same query is written once per dialect only where the SQL itself differs.
- **Holes `{expr}` are always bound parameters.** The compiler replaces each hole by the dialect's placeholder (`$1`, `@p1`, ...) and passes the value as a `db.Arg` chosen by the expression's static type (`int`, `long`, `float`, `bool`, `text`, `T?` as `NULL` when `nil`, `[]float` as a vector, or a `db.Arg` as it is). No value is ever spliced into the text. `{{` and `}}` write a literal brace. Holes are only read in code, not inside string literals, quoted identifiers or comments.
- **A dialect is a declaration, not compiler code.** A package that provides a dialect marks it with an attribute that states its lexical profile: placeholder style, string and identifier quoting, comment forms, dollar quoting, backslash escapes, whether one block may hold several statements. PostgreSQL (`postgres`) and T-SQL (`tsql`) are the first two; they live in the `postgres` and `sqlserver` packages. Oracle, MySQL, SQLite and others become new packages with a new profile and no compiler change. `sql <name>` finds the profile among the packages the file imports with `use`.
- **Compile-time checks** from the profile, without a server: unterminated string, quoted identifier, comment or dollar quote; unbalanced parentheses; a hole in a position where it would not be read; a literal placeholder written by hand; more than one statement where the dialect does not allow it; an unsupported hole type (the error names the type).
- **`db.Sql` runs through the existing connection.** `conn.rows_sql`, `conn.exec_sql`, `db.query_sql(T, conn, q)` and `db.one_sql(T, conn, q)` take a `db.Sql`. A connection knows its dialect name; running a statement written for another dialect is an error of kind `InvalidInput` that names both. Plain `text` statements keep working unchanged.
- The formatter keeps the lines of a `sql` block as written (re-indenting the block as a whole, like `asm`), `brasa.ebnf` and the docs describe it, and the VS Code grammar highlights SQL inside the block.

## Capabilities

### New Capabilities
- `sql-blocks`: the `sql <dialect>` block, holes as bound parameters, the dialect profile and its compile-time checks, and how a new dialect is added.

### Modified Capabilities
- `db-library`: `db.Sql`, the `*_sql` functions and methods, and the connection's dialect name.
- `postgres-driver`: provides the `postgres` dialect and sets the dialect name on its connections.
- `sqlserver-driver`: provides the `tsql` dialect and sets the dialect name on its connections.
- `formatter`: the lines of a `sql` block are kept as written.
- `vscode-extension`: SQL highlighting inside `sql` blocks.

## Impact

- `src/lexer.rs` (raw block mode, like `asm`), `src/parser.rs` and `src/ast.rs` (a block expression), the code generator (desugaring to a `db.Sql` value), `src/fmt.rs`, `brasa.ebnf`, `std/db.br`, `std/postgres.br`, `std/sqlserver.br`, `editors/vscode/syntaxes/brasa.tmLanguage.json`, `docs/databases.html`, `docs/syntax.html`, `examples/db.br`, new tests.
- No new keyword: `sql` is read as a block start only when it is followed by a dialect name and the end of the line, so existing identifiers named `sql` (parameters, variables) keep working.
- Not a breaking change; existing `conn.exec(text, ...)` code is unchanged.
- Non-goals for this change: a SQL parser or type checker against a schema, query builders, identifier or fragment splicing (`{ident(x)}`), embedding `.sql` files, and the Oracle, MySQL and SQLite drivers themselves (only their room in the design).
