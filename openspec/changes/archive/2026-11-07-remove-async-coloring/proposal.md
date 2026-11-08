## Why

Today `await` is accepted only inside an `async func` and inside `main` (`zila.ebnf` semantic rules, `src/codegen_x64.rs` `FnState::allow_await`, spec `async-execution` "Where await is allowed"). That splits every program into two worlds: a plain function cannot call anything that waits, so waiting is contagious and every caller up the chain must become `async`. The split also blocks ordinary generic code: `list.map(f)` cannot take a function that awaits, so a program cannot write `urls.map(func(u) await fetch(u) end)`.

The restriction is not needed by the runtime. Every task, including the main program (task 0, created by `task.init()` from the thread's own stack), has its own stack and is switched by `task.park()`; `future.wait` already works wherever it is called. The rule is purely a compile-time check.

## What Changes

- **BREAKING (relaxation, no existing program stops compiling):** `await` is allowed in any function, method, lambda or `defer`red call. It suspends only the current task on the cooperative scheduler and stays mandatory and visible at each wait point: there is no implicit waiting.
- `async func` stays, as the short form for "run this call as a task and return `Future<T>`". A function no longer needs `async` in order to contain `await`.
- `await` is rejected, with a clear compile-time error, in the two places where switching tasks is unsafe: a `drop` method and a function exported to C or passed as an `extern` callback (see design.md).
- The compiler drops `allow_await` / `next_lambda_await`; the check that remains is the platform one (bare-metal kernel).
- `async` functions keep taking no `ref` parameters (their lifetime question is covered by `add-restrict-ref-escape`).
- Docs (`docs/async.html`, `docs/syntax.html`, `docs/concurrency.html`), `zila.ebnf`, `examples/async.br`, `README.md` and `tests/async.rs` are updated.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `async-execution`: "Where await is allowed" is replaced by a rule that allows `await` in any function and states the forbidden contexts; the scheduling requirement says a task also switches at an `await` in any function it runs.
- `async-io`: the awaitable library operations are usable from any function (new requirement).

## Impact

`src/codegen_x64.rs` (`allow_await`, `next_lambda_await`, `ExprKind::Await`, `drop`/export checks), `src/regalloc.rs` (unchanged behavior, noted), `zila.ebnf`, `examples/async.br`, `tests/async.rs`, `docs/async.html`, `docs/syntax.html`, `docs/concurrency.html`, `README.md`. `std/task.br`, `std/future.br` and the `.linux` variants need no behavior change (comments only).
