## 1. Assertions

- [x] 1.1 `assert(cond)`, `assert(cond, "message")` and `assert_eq(a, b)` in the code generator, as rewrites into `if` and `return error(...)`, with the `line N:` prefix, single evaluation, and the error for functions that do not return an error
- [x] 1.2 Tests of the assertions (messages for ints, floats, bools and text, with and without a message, helpers with `try`, evaluated once, the compile error, a program function named `assert` takes precedence)

## 2. The runner

- [x] 2.1 `src/testrun.rs`: find `#[test]` functions, validate them, generate the entry point, build and run each file, forward the output
- [x] 2.2 Early stop detection (crash or panic), exit statuses, `no tests in <file>`, several files, `--no-opt`
- [x] 2.3 `--test` in `src/cli.rs` and `src/main.rs` (usage text, flag list)

## 3. Tests, example and documentation

- [x] 3.1 `tests/testrun.rs`: every scenario of the spec (passing and failing, invalid tests, own `main`, crash, several files, no tests)
- [x] 3.2 `examples/testing.br`
- [x] 3.3 `docs/getting-started.html` (the `--test` option), `docs/syntax.html` (`#[test]`, `assert`, `assert_eq`), README rows
- [x] 3.4 Full test suite (async skipped) and `openspec validate add-zila-test`
