## Why

Brasa code is formatted by hand, and the style drifts: indentation, spaces after commas, blank lines, the layout of `asm` blocks. Every language that wants a shared style ships a formatter so that nobody argues about it and diffs stay about meaning. `brasa fmt` is the first item of the tooling step of the roadmap.

## What Changes

- A new option `--fmt` of `brasa-parse`: `brasa-parse --fmt file.br [more.br ...]` rewrites each file in the standard style, and `brasa-parse --fmt-check file.br ...` changes nothing, lists the files that are not formatted and exits with 1 when there are any (for CI).
- The style (the formatter changes only whitespace and never the program): four spaces per block level (`func`, `if`/`else`, `while`, `for`, `match`, `unsafe`, `asm`, `record`/`enum`/`interface`, lambdas), the closing `end` and `else` at the level of their opener, one extra level for a line that continues an expression, the spacing between tokens left as written (so columns aligned with spaces stay aligned), except none before `,` `)` `]` or after `(` `[`, and one after `,`, no trailing spaces, at most one blank line in a row, no blank lines at the start of a block, a newline at the end of the file, the file's own line ending (LF or CRLF) kept, comments kept (own-line comments take the indentation of the code that follows (inside the block when it is followed by `end`), a trailing comment is separated by at least two spaces), `asm` lines indented as a block with their relative indentation kept.
- The formatter works on the token stream, so the text of every token (number spelling, string escapes, interpolations) is kept exactly. It never writes a file whose tokens would differ from the original's: it checks that before and after have the same tokens, reports an error and leaves the file untouched otherwise.
- The compiler's own `.br` files (`std/`, `examples/`, `os/`) are formatted with it, so the repository is the first user.

## Capabilities

### New Capabilities

- `formatter`: the style rules, the safety guarantees (same tokens, idempotent, comments kept) and the `--fmt` / `--fmt-check` options.

### Modified Capabilities

None.

## Impact

- `src/lexer.rs` keeps comments and the extent of every token (new function that returns them; the normal lexing is unchanged), `src/parser.rs` records which tokens open and close blocks, a new `src/fmt.rs`, `src/cli.rs` and `src/main.rs` (the two options).
- `tests/fmt.rs`, `docs/getting-started.html` (the flag list, which `tests/docs.rs` checks), README, and the reformatting of the repository's `.br` files (with `docs` regenerated if examples change).
