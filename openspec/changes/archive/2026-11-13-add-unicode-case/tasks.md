## 1. Mapping

- [x] 1.1 `std/text.zila`: `upper_cp` and `lower_cp` (Latin-1, Latin Extended-A, Latin Extended Additional, fullwidth Latin, Greek, Cyrillic, Armenian) and the new `upper`/`lower` with the ASCII fast path; verify with the tests of group 2
- [x] 1.2 Update the header comment of `std/text.zila` (it says `upper` and `lower` only change ASCII letters)

## 2. Tests

- [x] 2.1 `tests/text_case.rs`: the scenarios of the spec (ASCII, `ação` and `ÀÉÎÕÜ`, Cyrillic, Greek, `Straße 1`, invalid bytes do not abort); verify the suite passes
- [x] 2.2 `tests/text_case.rs`: a Zila program maps every code point of the supported blocks and the output equals Rust's `to_uppercase`/`to_lowercase` wherever those give one character; verify it passes, and that it fails when one range of `upper_cp` is changed
- [x] 2.3 Existing tests that use `text.upper`/`text.lower` (`comptime`, `ai`, `mcp`, `text_char`, `stdlib`) still pass; the old scenario "Straße 1" gives `STRAßE 1`

## 3. Documentation

- [x] 3.1 `docs/stdlib.html` row for `upper`/`lower` and `docs/async.html` or `examples/stdin_lines.zila` where it matters; `CHANGELOG.md` "Changed" (breaking for code that relied on ASCII only); verify `cargo test --test docs` passes

## 4. Wrap-up

- [x] 4.1 `cargo clippy --all-targets -- -D warnings` is clean and every test target except `async` passes; archive the change and sync the specs
