## Why

Zila Lang has no way to test Zila Lang code: the repository's own checks are Rust tests that compile whole programs and compare their output, and a library or program written in Zila Lang has nothing to run its checks with. A test runner is the next tooling item of the roadmap, and it also gives the standard library (written in Zila Lang) a natural way to test itself later.

## What Changes

- Functions marked `#[test]`, with no parameters and either no result or a result of `error`, are tests: `#[test] func adds() -> error ... end`.
- Two checking forms that work like `try`, as part of the language: `assert(cond)` and `assert(cond, "message")`, and `assert_eq(a, b)` for numbers, booleans, text and floats. A failed check ends the test by returning an error whose message names the line (`line 12: assertion failed: left 3, right 4`). They are only allowed in a function that returns `error` or `T or error` (a compile error says so otherwise), so a helper that checks things is written `-> error` and called with `try`.
- A new option `zila-parse --test file.br [more.br ...]` builds each file with a generated entry point that runs its tests in source order and prints `ok <name>` or `FAIL <name>: <message>` for each, then `test result: N passed, M failed`. The exit status is 1 when a test failed or a file does not compile, 0 otherwise. The file's own `main`, if any, is not used. A test that crashes the program (division by zero, a panic) aborts the run: the output so far is shown with a note naming the exit code. `--no-opt` works with `--test`.
- The option is native-only (it builds an executable), like `--build`.

## Capabilities

### New Capabilities

- `test-runner`: how tests are written (`#[test]`, `assert`, `assert_eq`), what `--test` prints and returns, and what is rejected.

### Modified Capabilities

None.

## Impact

- `src/codegen_x64.rs` (the `assert` forms as intrinsics, written as small rewrites into `if` and `return error(...)`), a new `src/testrun.rs` (finding tests, generating the entry point, building and running), `src/cli.rs` and `src/main.rs` (the `--test` option), `tests/testrun.rs`, `examples/testing.br`, `docs/getting-started.html` (the option list, which `tests/docs.rs` checks), `docs/syntax.html`, README.
