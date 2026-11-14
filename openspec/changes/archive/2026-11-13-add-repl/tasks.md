## 1. Running with captured output

- [x] 1.1 `src/run.rs`: share the build step between `run_file_with` and a new `run_captured` returning the build error or `(status, stdout, stderr)`; verify `cargo test --test run` and `--test eval` still pass and a unit or integration test shows the captured output

## 2. The session

- [x] 2.1 `src/repl.rs`: `Session` (accepted entries, bytes already shown), `check(entry)` returning complete, incomplete or a parse error (via `eval::wrap` and the parser), the expression heuristic with the `print(...)` first and statement fallback, and the output delta; verify with unit tests for each (incomplete `if`, open parenthesis, a real syntax error, `1 + 2`, `print("x")`, an assignment, the delta slice)
- [x] 2.2 The loop: read entries from standard input, prompts only on a terminal, continuation prompt, `:help`, `:show`, `:reset`, `:quit`/`:exit`, unknown `:` commands, end of input, failed entries discarded with the error naming `<repl>:N`; verify with the tests of group 4

## 3. Command line

- [x] 3.1 `src/cli.rs`: the bare word `repl` and the flag `-i` (mode `Repl`, added to `FLAGS`, no file needed); `src/main.rs`: usage text and dispatch; verify the cli parse tests, including that every flag is accepted

## 4. Tests

- [x] 4.1 `tests/repl.rs`: each scenario of the `repl` spec through `zila repl` with piped input (statement, piped script, variable and function across entries, no repeated output, expressions, a void call, a multi-line `if`, a type error then a good entry, undefined name after a failed entry, `:reset`, `:show`, unknown command, `:quit` and end of input give status 0); verify the suite passes

## 5. Documentation

- [x] 5.1 `docs/getting-started.html`: document `repl` and `-i`, the commands, and the limit (each entry runs the whole session again; side effects repeat); verify `cargo test --test docs` passes
- [x] 5.2 `README.md` (Getting started and feature list), `CHANGELOG.md` "Added"; verify `cargo test --test docs` still passes

## 6. Wrap-up

- [x] 6.1 `cargo clippy --all-targets -- -D warnings` is clean and every test target except `async` passes
- [ ] 6.2 Try the REPL by hand on a real terminal (prompt, continuation prompt, Ctrl-Z/Ctrl-D, accents), then archive the change and sync the specs (`repl` new)
