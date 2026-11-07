## Why

Brasa says it has no exceptions, yet it has `panic`: a call that stops the program with a message, plus a handful of run-time failures (index out of bounds, division by zero, a missing map key, running out of memory) that behave like it. The word promises more than the implementation does. There is no unwinding, no recovery, and `defer` and `drop` do not run, so `panic` is really "abort" under a name that makes people expect Rust or Go semantics. Meanwhile the grammar still says integer overflow "panics in debug", which no compiler path implements (arithmetic already wraps), and there is no way to ask for a list element without risking a fatal failure.

This change gives each kind of failure exactly one home:

1. Integer overflow is never a failure: arithmetic wraps, always.
2. Failures a caller can handle are values: `xs.get(i)` returns `T?`, and the library offers `checked_*` arithmetic returning `T or error`.
3. Programmer bugs are `abort`: the process prints one line to stderr and exits at once.

## What Changes

- **BREAKING**: `panic("message")` is removed from the language. A new built-in `abort("message")` is the only way a program stops fatally on purpose.
- **BREAKING**: a fatal stop prints `abort: <file>.br:<line>: <message>` instead of `panic: <message>`, and goes to standard error. The exit status stays 101.
- Fatal stops are specified once, as `abort`: out-of-bounds index, integer division or remainder by zero, a missing map key with `m[key]`, `take` from an empty list, `int(x)` of an out-of-range float, a failed `assert` outside `--test`, out of memory, and `abort(...)` itself. They run no `defer`, no `drop`, no unwinding, and cannot be caught.
- Integer arithmetic wraps at the width of the type in every build; the "panic in debug" rule is removed from `brasa.ebnf`. `MIN / -1` wraps to `MIN` instead of faulting.
- New library functions `checked_add`, `checked_sub`, `checked_mul`, `checked_div` return `int or error` (kind `Overflow` for overflow, `InvalidInput` for a zero divisor).
- New list method `xs.get(i)` returns `T?` (`nil` when `i` is out of range, negative included) for element types that can be nil, and `T or error` (kind `InvalidInput`) for integers, floats and booleans, which have no `nil` (optional scalars do not exist in the language yet). `Map.get(key, default)` is unchanged.
- `assert` and `assert_eq` keep their behavior under `--test` (the enclosing function returns the error and the test fails). In a normal build they abort. They are no longer a compile error in functions that do not return `error`.
- `std/text.br` `split` with an empty separator aborts through `abort(...)` instead of `panic(...)`.
- Standard library, `os/`, examples, tests and docs are rewritten where they say `panic`.

## Capabilities

### New Capabilities
- `abort`: what an abort is and when it happens, the `abort` function, the message format and exit status, wrapping integer arithmetic, `checked_*` arithmetic, and list `get`.

### Modified Capabilities
- `defer`: deferred calls do not run on an abort (wording: "panic or fatal error" becomes "abort").
- `floating-point`: `int(x)` out of range aborts.
- `linux-parity`: division by zero aborts with the new message.
- `linux-target`: exit codes, message format and out of memory use `abort`.
- `maps`: a missing key with `m[key]` aborts.
- `test-runner`: assertions return errors only under `--test` and abort otherwise; a crashed test run is described as an abort.
- `optimizer`: run-time failures inside inlined bodies stay aborts with the same message and code.
- `text-library`: `split` with an empty separator aborts.

## Impact

- **Compiler** (`src/codegen_x64.rs`): `panic` built-in replaced by `abort`; `rt_panic` becomes `rt_abort` (Windows, Linux and bare-metal runtimes) and writes to stderr; per-site location text for index, division, map-key, `take`, `int(float)` and `assert`; `assert_stmt` splits on test mode; `get` list method; `MIN / -1` handled. `src/runtime_map.asm` calls `rt_abort`.
- **Interpreter** (`src/interp.rs`): `panic` becomes `abort`. This file is removed by `remove-interpreter`; whichever lands first adapts (see design).
- **Test runner** (`src/testrun.rs`): `--test` must tell the compiler it is building a test entry point; an abort is reported as a crash.
- **Library**: `std/text.br` (the only `panic(` in `std/`, `os/` and `examples/`); new `checked_*` in `std/math.br`.
- **Grammar and docs**: `brasa.ebnf`, `docs/syntax.html`, `docs/native.html`, `docs/stdlib.html`, `README.md`.
- **Tests**: `tests/floats.rs`, `tests/linux.rs`, `tests/maps.rs`, `tests/optimizer.rs`, `tests/testrun.rs`, `tests/stdlib.rs` and new abort tests. Rust `panic!` in test helpers is Rust, not Brasa, and stays.
- **Assumptions**:
  - Exit status 101 is kept because `linux-target` and `linux-parity` and their tests already promise it.
  - "Forced nil" has no syntax in the language today (no `x!`); when a force-unwrap is added it aborts under this change's rules. No work is needed now.
  - Out of memory is detected inside the allocator, which has no source line; it prints `abort: out of memory` without a location (see design).
