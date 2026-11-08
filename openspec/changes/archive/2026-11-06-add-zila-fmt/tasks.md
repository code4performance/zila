## 1. Lexer and parser support

- [x] 1.1 `Token` gets a `span` (char offsets), and `tokenize_all()` returns the tokens and the comments (`tokenize()` unchanged)
- [x] 1.2 The parser records block pairs (opener, `end`) and `else` tokens, readable after parsing (`parse_with_layout`), without changing existing callers
- [x] 1.3 Tests that the recorded pairs are right for every construct (func, lambda, if/else if/else, while, for, match, unsafe, asm, record, enum, interface) and that function types and `extern func` record nothing

## 2. The formatter

- [x] 2.1 `src/fmt.rs`: line layout, indentation (blocks, `end`/`else`, continuation lines), spacing within a line, blank lines
- [x] 2.2 Comments (own line, trailing, block) and `asm` block re-indentation
- [x] 2.3 The safety net (same tokens before and after), line ending preservation, final newline
- [x] 2.4 `--fmt` and `--fmt-check` in `src/cli.rs` and `src/main.rs` (files rewritten or listed, exit status, parse errors)

## 3. Tests

- [x] 3.1 `tests/fmt.rs`: one test per scenario of the spec (nested blocks, function types, lambdas, continuation lines, commas and brackets, blank lines, comments, literals, asm, CRLF, broken file)
- [x] 3.2 Every `.br` file of `std/`, `examples/`, `os/` keeps its tokens and is idempotent; scrambling the indentation of those files (the ones without `asm` or block comments) and formatting again gives the same text
- [x] 3.3 CLI tests for `--fmt` and `--fmt-check` (changed files, exit codes)

## 4. The repository and documentation

- [x] 4.1 Format `std/`, `examples/` and `os/` with `--fmt`, regenerate the docs examples (`--docs`) if needed, and show `--fmt-check` is clean
- [x] 4.2 Describe `--fmt` and `--fmt-check` in `docs/getting-started.html`, the README tooling row and the usage text
- [x] 4.3 Full test suite (async skipped) and `openspec validate add-brasa-fmt`
