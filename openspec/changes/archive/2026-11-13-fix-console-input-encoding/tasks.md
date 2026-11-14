## 1. Fix

- [x] 1.1 `std/fs.zila`: declare `SetConsoleCP` and call it with 65001 in `stdin()`; verify the program builds and a piped UTF-8 line (`ação`) is read intact
- [x] 1.2 `tests/stdin.rs`: a test that pipes `ação` and checks `read_line()` returns it and that `text.upper` on it does not abort; verify the suite passes
- [ ] 1.3 Check by hand in a Windows terminal with a non-UTF-8 input code page that `ç` typed at `zila run examples/stdin_lines.zila` no longer aborts (a console cannot be driven by the test harness)

## 2. Documentation

- [x] 2.1 `docs/async.html` says a terminal's input is switched to UTF-8 on Windows; verify `cargo test --test docs` passes
- [x] 2.2 `CHANGELOG.md` "Fixed"; verify the docs tests still pass
