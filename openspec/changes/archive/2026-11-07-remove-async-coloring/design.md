## Context

`await e` lowers to `future.wait(e)` (`ExprKind::Await` in `src/codegen_x64.rs`). `wait` loops on `fut.done`, registers `task.current` in `fut.waiters` and calls `task.park()`, which marks the task waiting and runs `switch_away()`, the scheduler loop. The only restriction is the compile-time flag `FnState::allow_await`, set to `fd.is_async || (fd.name == "main" && m == main_mod)` in the function prologue and to `next_lambda_await` for the lambda that is the body of an `async func` (`emit_async_launch`).

How the pieces work today (`std/task.br`; `std/task.linux.br` is the same code on `mmap` stacks, assembly switches and `epoll`):

- A task is a fiber with its own stack (256 KiB, guard page). `tasks[0]` is the main program: `task.init()` runs `ConvertThreadToFiber` (Linux: saves the thread context) and stores the thread's own stack as task 0. `init()` is called by `start`, `yield`, `chan_new`, `chan_recv` and `future.wait/wait_any/timeout_wait/sleep_future`, so it always runs before the first switch.
- `switch_away()` is the scheduler loop. It fires due timers, polls I/O completions every 64 switches, takes the next id from `ready` and switches to it. With nothing ready it waits for the earliest timer or the completion port; with no timers and no pending I/O it calls `fatal("all tasks are asleep - deadlock!")`. It runs on the stack of the task that is giving up the processor; there is no separate scheduler stack.
- `async func f()` compiles its body into a closure and calls `future.launch`, which does `task.start(...)`; the future is completed at the end of that task.

## Findings (does await outside a task need the main program to be a task?)

The case "await outside a task" does not exist: every piece of running code is on some task's stack, and the main program is task 0. `park()` from task 0 sets `tasks[0].waiting = true` and switches to another fiber; when the awaited future completes, `wake(0)` re-queues task 0 and a later switch resumes it exactly where `park` left it, with the frames of `main` and of every function it called intact on the thread stack. A plain function is the same: a call frame is part of the current task's stack. So no change to stacks, fibers, the scheduler loop or `std/future.br` is needed. `await` in a plain function already works at run time; the compile-time rule is the only barrier. Nested awaits are ordinary nested calls on one stack.

Checked in the code:

- Deadlock is detected the same way whoever waits: with `main` parked and nothing able to wake it, `switch_away` finds an empty ready queue, no timer and no pending I/O and stops with the deadlock message and a non-zero code.
- Termination: `main` returning ends the program even while other tasks wait. Unchanged.
- Registers: `regalloc::plan` keeps no variable in a register in a function that contains an `await` (`ExprKind::Await` sets `blocked`). A plain function that calls a function that awaits keeps its register variables across the call, which is safe because the fiber switches preserve each task's callee-saved registers. No change.
- Reference counting is per value, not per task; a suspended frame keeps its locals alive until it resumes. `fiber_main` still must not own heap values (it never returns); nothing awaits there.
- Test runner (`src/testrun.rs`): a `test_` function is called like a plain function, so it may contain `await`; `async` test functions stay rejected.

## Decisions

1. **Remove the check, keep the keyword.** Delete `FnState::allow_await` and `next_lambda_await`; the `ExprKind::Await` arm keeps the Future-type check, and the bare-metal rejection of `future` stays. `await` remains written at every wait point, so suspension points stay greppable and there is no implicit waiting.
2. **`async func` stays as sugar.** It means "run the body as a task and return `Future<T>`". That differs from "may await": a plain function that awaits suspends its caller's task, while an `async func` starts concurrency. `emit_async_launch` is unchanged except for no longer setting `next_lambda_await`. `async` functions still take no `ref` parameters (the task outlives the call and works on copies); that rule belongs to `add-restrict-ref-escape` and its diagnostic is untouched here.
3. **Lambdas may await.** A lambda runs on the stack of whoever calls it, so `list.map(func(x) await f(x) end)` suspends the task running `map`; `map` needs no change and does not become async. The lambda's result type is the awaited type, so `map` yields `[]T`. To start all the calls first and collect them afterwards, write `await all(xs.map(f))` with an `async func f`; the docs show both forms.
4. **`defer` may await.** The deferred call is emitted in the function's own epilogue on the task's stack (`defer_stmt`), so it behaves like an ordinary call at exit and delays the return.
5. **`drop` methods may not await (compile error).** `drop` runs inside `rt_release` / `B_drop_*`, which can happen in the middle of an expression, an assignment, another `drop` or the finishing of a task (`tasks[me].action = nil` in `fiber_main`). Parking there would let other tasks observe objects half released. Only a direct `await` in the body is detected; an indirect one through a callee is the author's responsibility and is documented. Waiting for I/O before release is done with an explicit awaited `close()`.
6. **C callbacks may not await (compile error).** A function marked `#[export]`, or converted to an `extern [abi] func(...)` type (window procedures in `std/ui.br`, the DNS completion routine `task.dns_done`), is called by foreign code, and a fiber switch would suspend a C frame. A direct `await` in such a function is rejected at the conversion/export. Indirect awaits are not detected; the docs tell callbacks to `send` on a channel or wake a task instead.
7. **Main thread.** `main` awaits on the thread's own stack as task 0. That stack is the process stack, not a 256 KiB task stack. `task.init()` already runs before the first switch, so the generated entry point needs no explicit initialization.
8. **Scheduler loop unchanged.** No dedicated scheduler stack: the loop runs on the stack of the parking task, valid for task 0 and plain functions alike. A separate scheduler context would add cost without fixing a failure.
9. **Interpreter.** `src/interp.rs` still answers "'await' needs the native compiler"; `remove-interpreter` deletes it.

## Depends on

- `add-direct-pe-output`: only the build path used by the new tests (`tests/async.rs` builds natively); no overlap in scope.
- `add-restrict-ref-escape`: owns the `ref`-in-async rule; left untouched.
- `remove-interpreter`: removes the `ExprKind::Await` arm of `src/interp.rs`; order does not matter.
- `add-threads-and-shared`: if each thread gets its own scheduler, "the current task" in `await` means the task on that thread's scheduler; the rules here hold per thread.
- `add-error-kind`, `add-copy-on-write`: touch the same tests and docs files; rebase if they land first.

## Risks

- A function that waits looks like any other at the call site (that is the point of removing coloring). Mitigation: `await` stays in the body, the docs say a plain function containing `await` suspends its caller's task.
- Indirect `await` in `drop` or C callbacks is not detected. Mitigation: documented; the direct cases are errors.
- Reentrancy: two tasks can be inside the same plain function at once (already true for `async`). Globals touched across an `await` are shared; the docs call it out.
