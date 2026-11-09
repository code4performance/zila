## 1. Wrapping snippets

- [x] 1.1 Add `src/eval.rs` with `wrap(snippet) -> Wrapped { source, line_map }` implementing the line rule from design.md (declarations vs statements, `use` first, own `func main` skips wrapping, `package main` added when missing); verify with unit tests for: a one-liner, declarations mixed with statements, `use` lines, a nested block inside a declaration, a snippet with its own `main`, and an empty snippet
- [x] 1.2 Add `map_error` that rewrites a diagnostic position of the wrapped program to the snippet's line and column (indentation subtracted); verify with a unit test for an error on line 2 and for an error in generated text

## 2. Command line

- [x] 2.1 `src/cli.rs`: modes `Eval` and the bare word `run` (setting `Mode::Run`), option `-e <code>` and the file `-` for standard input; add `-e` to `FLAGS`; verify the parse tests (`zila eval "x"`, `zila -e x`, `zila run f.zila --no-opt`, `zila run -`, `zila eval -`, missing code is an error, `--run` still parses) and the existing test that every flag is exercised
- [x] 2.2 `src/main.rs`: usage text lists `run`, `eval` and `-e`; dispatch reads the program from the file, the argument or standard input, wraps it for `eval`, labels errors `<eval>` or `<stdin>`, and calls the same path as `--run` (host target check included); verify by running each form by hand and with the tests of group 3

## 3. Tests

- [x] 3.1 `tests/eval.rs`: run the built `zila` for each scenario of the `eval-command` spec (one-liner prints `3`, declarations plus statements, `use text`, failed `try` gives status 1 and the program's error line, complete program inline, here-document through standard input, a whole program through `run -`, errors name the snippet's lines and never `main.zila`, `run` equals `--run`, another target is refused); verify the suite passes (the line-map assertions fail without the map, since the message would name `main.zila`)
- [x] 3.2 `tests/run.rs` keeps passing unchanged, and `tests/eval.rs` checks that `zila run <file>` and `eval` leave nothing behind in the temporary directory

## 4. Documentation

- [x] 4.1 `docs/getting-started.html`: document `run`, `eval`, `-e` and `-`, with the wrapping rule and the here-document example; verify `cargo test --test docs` passes (it fails if `-e` is undocumented)
- [x] 4.2 `README.md` "Getting started": add `zila -e 'print("hi")'` and `zila run file.zila`; `CHANGELOG.md` Unreleased "Added"; verify `cargo test --test docs` still passes

## 5. Wrap-up

- [x] 5.1 `cargo clippy --all-targets -- -D warnings` is clean and every test target except `async` passes; archive the change and sync the specs (`eval-command` new, `run-command` updated)
