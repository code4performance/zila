## Why

`defer` is in the grammar and in the documentation, but the native compiler rejects it and the interpreter silently ignores it. That leaves a gap in the language's own story about resources: files, sockets and locks should be released on every way out of a function, including an early `return` and a failed `try`. Records can already clean up with `drop`, but that ties cleanup to a type; programs also need to say "when this function ends, do this" for things that are not a type of their own (print a log line, release a lock, flush a buffer, restore a setting).

## What Changes

- The native compiler supports `defer call(...)`: the call runs when the enclosing function ends, whichever way it ends (`return`, running off the end, or a failed `try`), in the reverse order the `defer` statements ran.
- The variables a deferred call uses are captured when the `defer` statement runs. Records are shared rather than copied, so a deferred `buf.flush()` flushes the live buffer.
- A deferred call must be a call. Anything else is a compile error that says so.
- A deferred call that can fail has its error ignored.
- The tree-walking interpreter (`--run`) runs deferred calls too, instead of skipping them.
- Documentation, an example program and the README status are updated in the same change.

## Capabilities

### New Capabilities
- `defer`: running a call when the enclosing function ends, its ordering, what it captures, and where it does and does not run.

### Modified Capabilities
<!-- None: there is no earlier spec of this behavior. -->

## Impact

- **Compiler** (`src/codegen_x64.rs`): a per-function list of deferred closures, run at every function exit; closures that share records instead of copying them.
- **Interpreter** (`src/interp.rs`): a frame of deferred calls per function call.
- **Documentation**: `docs/syntax.html`, `docs/native.html`, `docs/examples.html` (new example `examples/defer.br`), `README.md`.
- **Assumptions**:
  - `defer` is scoped to the **function**, as in Go, not to the enclosing block. A `defer` in a loop runs once per iteration, at function exit.
  - Deferred calls do not run when the program stops abruptly (a runtime panic such as an index out of bounds, or `fatal`), because nothing is unwound then.
  - `break` and `continue` do not run deferred calls; only leaving the function does.
  - Because the variables are captured when `defer` runs, a deferred call cannot assign to them (closures capture by value). It can call methods that change shared records.
