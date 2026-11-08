## 1. Syntax and the block scanner

- [x] 1.1 Lexer: read a line that is an identifier `sql`, one identifier and the end of the line as the start of a raw block; collect raw lines until a line that is only `end`; keep `sql` an ordinary identifier everywhere else; report an unterminated block with its start line
- [x] 1.2 AST and parser: a block expression with the dialect name, the raw lines and their positions; add `sql_block` to `zila.ebnf`
- [x] 1.3 Add `src/sqlblock.rs`: the profile-driven scanner (code, string, identifier, line comment, block comment with optional nesting, dollar quote), hole extraction with `{{`/`}}` escapes, placeholder rendering for `$n`, `@pn`, `:n`, `?`, and the diagnostics of the spec (unterminated constructs, unbalanced parentheses, hand-written placeholder, extra statements, empty block, trailing `;` removed)
- [x] 1.4 Unit tests of the scanner with adversarial input (quotes in comments, `--` in strings, nested comments, `$$` in identifiers, brackets in `tsql` and in `postgres`, `::` casts, `@variables`)

## 2. Dialect declarations

- [x] 2.1 Read the `#[sql_dialect]`, `#[sql_placeholder]`, `#[sql_string]`, `#[sql_ident]`, `#[sql_line_comment]`, `#[sql_block_comment]`, `#[sql_dollar_quote]`, `#[sql_backslash]` and `#[sql_multi]` attributes of imported packages into profiles; resolve `sql <name>` among the imports; errors for an unknown dialect, a known one that is not imported (name the package) and a duplicate name
- [x] 2.2 Declare `postgres` in `std/postgres.br` and `tsql` in `std/sqlserver.br` with the profiles of the specs, including the `GO` rejection for `tsql`
- [x] 2.3 Check the standard library still compiles with pruning on and that an imported package that declares a dialect but is never used adds nothing to the program

## 3. Code generation and the library

- [x] 3.1 `std/db.br`: `db.Sql` (dialect, text, args), `Conn.dialect`, an optional trailing dialect parameter on `db.connection`, and `rows_sql`, `exec_sql`, `query_sql`, `one_sql` with the `InvalidInput` dialect check
- [x] 3.2 Set the dialect name in `postgres.open` and `sqlserver.open`
- [x] 3.3 Code generator: desugar a block to a constant `db.Sql` when it has no holes, otherwise to a `db.Sql` with an argument list built from the holes converted by static type (integers, floats, `bool`, `text`, optionals, `[]float`, `db.Arg`); a clear error for any other type naming it and the line

## 4. Tooling

- [x] 4.1 Formatter: keep the lines of a `sql` block as written, moved as one unit with relative indentation kept, idempotent; tests next to the `asm` formatter tests
- [x] 4.2 VS Code grammar: SQL region from the `sql <dialect>` line to `end`, holes as Zila Lang expressions, no region for an identifier `sql`; extend `tests/vscode.rs` or the extension tests
- [x] 4.3 Language server: no new diagnostics beyond the compiler's; verify the existing syntax-error report still works with a block in the file and does not flag SQL text

## 5. Tests

- [x] 5.1 `tests/sql_blocks.rs`: every scenario of `sql-blocks` that needs no server (asm shape: placeholder text per dialect, constant statement without holes, optional hole as `NULL`, error messages and lines, dialect resolution errors, `sql` as an identifier)
- [x] 5.2 Server tests in the style of `tests/postgres.rs` and `tests/db.rs`, on the developer's local PostgreSQL and SQL Server (not Docker, as for the existing database tests): same function on both dialects, injection text as data, `$$` text, `@variables`, optionals, a transaction with rollback, wrong-dialect error that sends nothing
- [x] 5.3 Run the whole suite (all tests are allowed) and fix regressions

## 6. Documentation and examples

- [x] 6.1 Update `docs/databases.html` (the block, holes, the two dialects, the dialect check, how another database would add one) and `docs/syntax.html` (the block next to `asm`), and rewrite `examples/db.br` to use the blocks for the two dialects; run `zila-parse --docs` and `cargo test --test docs` to verify the pages, the ebnf rule and the code samples
- [x] 6.2 Update the README database section and the "how Zila Lang will evolve" list to say Oracle, MySQL and SQLite are planned as new packages with a dialect declaration, and note in the docs that dialects are lexical profiles, not SQL parsers
