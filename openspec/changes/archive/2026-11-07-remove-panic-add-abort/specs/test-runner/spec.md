## MODIFIED Requirements

### Requirement: Assertions
Under `--test`, `assert(cond)` and `assert(cond, "message")` SHALL do nothing when `cond` is true and otherwise SHALL return from the enclosing function with an error whose message begins `line N:` (N being the line of the assertion) and continues with `assertion failed` or the given message. `assert_eq(a, b)` SHALL evaluate each argument once, compare integers, booleans, floats and text of the same kind, and on a difference SHALL return an error `line N: assertion failed: left X, right Y` with the two values. In a normal build (outside `--test`) a failed assertion SHALL abort with the same text instead of returning an error. An assertion in a function that does not return `error` or `T or error` SHALL be accepted by the compiler; under `--test` a failure there SHALL abort, so the run stops early. A function of the program named `assert` or `assert_eq` SHALL take precedence over the built-in form.

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

#### Scenario: Outside --test
- **WHEN** a normally built program runs `assert(false)` in a function that returns nothing
- **THEN** it compiles, and at run time it aborts with `abort: <file>.br:<line>: assertion failed` and exits with 101

#### Scenario: Not in an error function
- **WHEN** `assert(true)` is used in a function that returns nothing
- **THEN** compilation succeeds

#### Scenario: Failure in a function without an error result under --test
- **WHEN** a test calls a helper that returns nothing and whose `assert` fails
- **THEN** the run stops early as for any abort, with exit status 1 from `--test`

### Requirement: Running tests
`zila-parse --test file.br [more.br ...]` SHALL build each file with an entry point that runs its tests (the file's own `main` is not used), print `ok <name>` or `FAIL <name>: <message>` for each test in order, and then print `test result: N passed, M failed`. It SHALL exit with 1 when any test failed, a file does not parse or compile, or the program stopped before finishing (a crash or an abort, reported with its exit code after the output so far), and with 0 otherwise. A file without tests SHALL print `no tests in <file>` and SHALL NOT make the status 1. `--no-opt` SHALL be honoured.

#### Scenario: Passing and failing tests
- **WHEN** a file has one passing and one failing test
- **THEN** the output has `ok` for the first, `FAIL` with the message for the second, `test result: 1 passed, 1 failed`, and the exit status is 1

#### Scenario: A crashing test
- **WHEN** a test divides by zero
- **THEN** the output so far is shown, followed by a note that the run stopped early with the exit code (an abort, 101), and the exit status is 1

#### Scenario: A file's own main
- **WHEN** the file also defines `main`
- **THEN** `main` is not run

#### Scenario: Several files
- **WHEN** two files are given
- **THEN** each is built and run in turn and the exit status is 1 if any of them failed
