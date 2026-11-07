## Purpose

Lets programs written in Brasa test themselves: `#[test]` functions, `assert` and `assert_eq`, and `brasa-parse --test` to run them and report.

## Requirements

### Requirement: Writing tests
A top-level function marked `#[test]` SHALL be a test. A test SHALL have no parameters, SHALL return nothing or `error`, and SHALL NOT be `async`, generic or a method; otherwise `--test` SHALL report the problem naming the function and run nothing in that file. Tests SHALL run in the order they appear in the file. Files with tests SHALL be in `package main`.

#### Scenario: A test that returns an error
- **WHEN** a file has `#[test] func adds() -> error` that ends with `return`
- **THEN** `--test` runs it and reports `ok adds`

#### Scenario: Not a valid test
- **WHEN** a function marked `#[test]` takes a parameter, or is `async`
- **THEN** `--test` reports an error naming the function and the exit status is 1

### Requirement: Assertions
`assert(cond)` and `assert(cond, "message")` SHALL do nothing when `cond` is true and otherwise SHALL return from the enclosing function with an error whose message begins `line N:` (N being the line of the assertion) and continues with `assertion failed` or the given message. `assert_eq(a, b)` SHALL evaluate each argument once, compare integers, booleans, floats and text of the same kind, and on a difference SHALL return an error `line N: assertion failed: left X, right Y` with the two values. The assertions SHALL be allowed only in a function that returns `error` or `T or error`; anywhere else the compiler SHALL report that `assert` needs a function that returns an error. A function of the program named `assert` or `assert_eq` SHALL take precedence over the built-in form.

#### Scenario: Failing assert_eq
- **WHEN** a test runs `assert_eq(1 + 2, 4)` on line 7
- **THEN** it fails with the message `line 7: assertion failed: left 3, right 4`

#### Scenario: assert with a message
- **WHEN** `assert(x > 0, "x must be positive")` fails
- **THEN** the message is `line N: x must be positive`

#### Scenario: Helpers
- **WHEN** a helper `func check(n: int) -> error` uses `assert` and a test calls `try check(1)`
- **THEN** a failure in the helper fails the test with the helper's message

#### Scenario: Evaluated once
- **WHEN** an argument of `assert_eq` is a call that prints
- **THEN** it prints once

#### Scenario: Not in an error function
- **WHEN** `assert(true)` is used in a function that returns nothing
- **THEN** compilation fails saying that assert needs a function that returns an error

### Requirement: Running tests
`brasa-parse --test file.br [more.br ...]` SHALL build each file with an entry point that runs its tests (the file's own `main` is not used), print `ok <name>` or `FAIL <name>: <message>` for each test in order, and then print `test result: N passed, M failed`. It SHALL exit with 1 when any test failed, a file does not parse or compile, or the program stopped before finishing (a crash or a panic, reported with its exit code after the output so far), and with 0 otherwise. A file without tests SHALL print `no tests in <file>` and SHALL NOT make the status 1. `--no-opt` SHALL be honoured.

#### Scenario: Passing and failing tests
- **WHEN** a file has one passing and one failing test
- **THEN** the output has `ok` for the first, `FAIL` with the message for the second, `test result: 1 passed, 1 failed`, and the exit status is 1

#### Scenario: A crashing test
- **WHEN** a test divides by zero
- **THEN** the output so far is shown, followed by a note that the run stopped early with the exit code, and the exit status is 1

#### Scenario: A file's own main
- **WHEN** the file also defines `main`
- **THEN** `main` is not run

#### Scenario: Several files
- **WHEN** two files are given
- **THEN** each is built and run in turn and the exit status is 1 if any of them failed

### Requirement: The option is documented
The getting-started page SHALL describe `--test`, and the syntax page SHALL describe `#[test]`, `assert` and `assert_eq`.

#### Scenario: Docs check
- **WHEN** the documentation test compares the options the tool accepts with the options the docs describe
- **THEN** `--test` is described
