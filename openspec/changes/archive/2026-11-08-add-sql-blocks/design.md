## Context

`asm` blocks are the model: the lexer switches to a raw mode at a line that is only `asm` and reads lines until a line that is only `end` (`src/lexer.rs`, `in_asm`; `src/parser.rs`, `asm_block`). SQL today goes through `db.Conn.exec/rows`, `db.query`, `db.one` with text and `db.Arg` values (`std/db.br`); drivers return a `db.Conn` built with `db.connection(run, begin_tx, commit_tx, rollback_tx, close_conn)`, and the placeholders in the text are the server's own. Attributes already carry string arguments (`#[link("user32")]`, `#[db("column")]`, `ast::Attr { name, args }`). `sql` is already used as an ordinary identifier in the repository (`examples/db.br`, tests), so it cannot become a keyword.

## Goals / Non-Goals

**Goals:**
- Write a SQL statement once in the form the server expects, with values only as parameters.
- Catch lexical mistakes at compile time, per dialect.
- Make adding a dialect (Oracle, MySQL, SQLite) a matter of a package and a declaration, not a compiler change.

**Non-Goals:**
- Parsing SQL grammar, checking tables or columns against a schema, query builders, splicing identifiers or fragments, `.sql` file embedding, translating between dialects, and the new drivers. Each is a possible later change.

## Decisions

1. **A block expression `sql <dialect> ... end`, contextual.** The lexer treats `sql` as a block start only when it is an identifier followed by one identifier and the end of the line (or an opening for a later `-> T` form). Everything else stays an identifier. Alternative considered: a keyword; rejected because it breaks existing code and tooling. Alternative: a string-literal form `sql"..."` ; rejected because multi-line SQL with quotes is the whole point.
2. **Lexing happens in the compiler, driven by a profile.** The block body is a list of raw lines in the token stream. A small profile-driven SQL scanner (`src/sqlblock.rs`) walks the text once and produces: the final statement text with placeholders, the hole expressions (each re-parsed by the normal expression parser with the line and column of the hole), and diagnostics. The scanner knows states (code, string, identifier, line comment, block comment, dollar quote) and nothing about SQL grammar. This keeps the compiler small, the behaviour the same for every dialect and the checks testable without a server.
3. **A dialect is a declaration with stacked attributes on a marker type.** In package `postgres`:

   ```
   #[sql_dialect("postgres")]
   #[sql_placeholder("$n")]
   #[sql_string("'")]
   #[sql_ident("\"")]
   #[sql_line_comment("--")]
   #[sql_block_comment("/*", "*/", "nested")]
   #[sql_dollar_quote]
   pub type Postgres = record end
   ```

   Keys: `sql_placeholder` is one of `"$n"`, `"@pn"`, `":n"`, `"?"`; `sql_string` and `sql_ident` list the quote pairs (a pair like `"[", "]"`); `sql_backslash` marks strings where backslash escapes (MySQL); `sql_multi` allows several statements per block. A new package declares its own marker and nothing else; `use` of the package is what makes the dialect visible. No Zila Lang code runs at compile time, so the existing comptime machinery and its cache are not involved. Alternatives: a `db.Dialect` interface evaluated by comptime (rejected: needs a driver program run on every build and cannot lex raw lines before comptime exists); a hard-coded table in the compiler (rejected: every database needs a compiler release).
4. **The compiler writes the placeholders, not the driver.** For each hole the compiler emits the profile's placeholder (`$1`, `@p1`, `:1`, `?`) and an argument. The result is `db.sql(dialect, text, [args])`, where `text` is a constant string and the argument list is built from the hole expressions. A block without holes is a constant and builds nothing at run time. Holes use a separate parameter each, even when the same expression is written twice, so numbering is trivial and identical for every style (including `?`).
5. **Hole conversion by static type.** The code generator knows the type of each hole expression and picks `db.int`, `db.float`, `db.bool`, `db.str`, `db.null` for `nil` of an optional, `db.vector`, or passes a `db.Arg` as is. Anything else is an error naming the type; there is no implicit `to_text`. Adding conversions later (dates, `[]u8` as bytes) is a compatible change.
6. **The connection learns its dialect name.** `db.Conn` gains a `dialect: text` field, set by `postgres.open` and `sqlserver.open`; `db.connection` keeps its signature and gets an optional trailing parameter, so test doubles keep working and accept any dialect. `*_sql` functions compare names and return `InvalidInput` before sending anything. Alternative: encode the dialect in the type (`Conn<Postgres>`) so the mismatch is a compile error; deferred, because it would change every signature that takes a `db.Conn` and breaks the "same code, two drivers" property the library is built on.
7. **New function names instead of overloading.** `rows_sql`, `exec_sql`, `query_sql`, `one_sql` are added; the text-based functions are unchanged. Reason: overloads on the type of the argument would complicate the checker and the error messages for a gain of four names. If the language gains overloading, these can become aliases.
8. **T-SQL specifics.** `@name` variables are ordinary text; only `{expr}` is a hole, and placeholders are `@p1..`. A `GO` line is rejected with an explanation. Several statements are allowed because `sp_executesql` accepts a batch. PostgreSQL allows one statement per block because the extended protocol does; a trailing `;` is removed in both.
9. **Where the room for other dialects is.** Oracle: `:1` placeholders, `"x"` identifiers, `q'[...]'` alternative quoting (needs a new profile key `sql_q_quote`, a compatible addition), no trailing `;`. MySQL: `?`, backtick identifiers, backslash escapes, `#` line comments (a second `sql_line_comment`). SQLite: `?`, `[x]`, `"x"` and backtick identifiers, `--`. The profile keys above are chosen so these need one new key at most. The drivers are separate changes; this change ends at `sql <name>` for the two it ships.
10. **Tooling follows the `asm` precedent.** The formatter treats the lines as a verbatim body re-indented as one unit (the first line sets the base; relative indentation kept), `zila.ebnf` gets a `sql_block` rule, the VS Code grammar embeds `source.sql` for the region and Zila Lang expressions for holes, and `docs/databases.html` and `docs/syntax.html` describe it. `tests/docs.rs` checks the ebnf rule, flags and code samples, so each sample must compile.

## Risks / Trade-offs

- A hole must never be read inside a string or comment, or an injection-looking quote could shift the scanner's state and change which text is a hole. The scanner is therefore a single left-to-right state machine over the raw text; tests include adversarial lines (`'` inside comments, `--` inside strings, nested comments, `$$` inside identifiers).
- Profiles are lexical only: they will accept SQL the server rejects. That is by design; the server stays the authority on grammar, and its error messages are unchanged.
- A dialect declared by an attribute on a type is unusual; the marker type is a deliberate place to attach it, and the same declaration can later carry methods if a runtime interface is wanted.
- `{expr}` inside SQL collides with a few uncommon constructs outside strings (for example some T-SQL JSON paths are inside strings and are not affected). `{{` and `}}` are the escape.
- Same-dialect only at run time: a program that mixes dialects finds out when the statement runs, not when it compiles (decision 6).

## Open Questions

- Should a block be allowed an optional result type (`sql postgres -> Customer`) so that `query_sql` need not repeat it? It does not change the first version; the grammar leaves room after the dialect name.
