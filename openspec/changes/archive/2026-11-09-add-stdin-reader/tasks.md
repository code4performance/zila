## 1. Library

- [x] 1.1 `std/fs.zila`: `GetStdHandle`, `Reader.std`, `stdin_read` (synchronous `ReadFile`, broken pipe is the end), `fs.stdin()`, and the `std` branch in `fill()`; verify with the tests of group 3 on Windows
- [x] 1.2 `std/fs.linux.zila`: the same with `read(0, ...)` retried on `EINTR`; verify `cargo test --test std_parity` passes with no new entry in `tests/data/std_parity_known.txt`, and the Linux tests of group 3 pass under WSL where available

## 2. Commands

- [x] 2.1 Check that `zila -e` and `zila run <file>` pass standard input through to `fs.stdin()` (no tool change expected) and that `zila run -` leaves an empty input; verify with the tests of group 3

## 3. Tests

- [x] 3.1 `tests/stdin.rs`: the scenarios of the `stdin-reader` spec (lines with `\n` and `\r\n`, `read_all`, a 200 000-byte redirected file, empty input, a last line with no newline, a slow producer, closing then calling `fs.stdin()` again) through `zila run` with a piped standard input; verify the suite passes
- [x] 3.2 `tests/eval.rs`: a filter written inline reads piped lines and prints them in upper case, and a program started with `run -` sees an empty input; verify the suite passes

## 4. Documentation

- [x] 4.1 `docs/async.html` (buffered reading) and `docs/stdlib.html` if it lists `fs`: document `fs.stdin()`, that it blocks the whole program, and the pipe example; add `examples/` program `stdin_lines.zila` and verify `cargo test --test docs` passes (examples embedded in the site match the files)
- [x] 4.2 `README.md` feature list and `CHANGELOG.md` "Added"; verify `cargo test --test docs` still passes

## 5. Wrap-up

- [x] 5.1 `cargo clippy --all-targets -- -D warnings` is clean and every test target except `async` passes; archive the change and sync the specs
