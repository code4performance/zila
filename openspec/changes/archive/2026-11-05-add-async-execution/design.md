## Context

Tasks today are Windows fibers scheduled by `std/task.br` (Brasa code): a ready queue, `switch_away()`, channels, and a deadlock check that fires whenever the ready queue is empty. Everything is single-threaded and cooperative. Heap values are reference counted, `T or error` is a boxed result, and standard libraries (`ui`, `task`) are Brasa files embedded in the compiler and compiled with the program; they reach the OS through `extern "system"`. See proposal.md for motivation and specs/ for required behavior.

## Goals / Non-Goals

**Goals:**
- `async`/`await` that reads simply and reuses what exists: fibers, the ready queue, `or error`/`try`, reference counting.
- Timers, files and TCP that never block other tasks.
- Almost all of it written in Brasa in the standard library, as `ui` and `task` are.

**Non-Goals:**
- Parallelism, thread pools, atomic reference counts.
- Cancellation, TLS, UDP, async DNS, directory APIs, `ui` message-loop integration.
- Generics in the language: the generic parts (`Future<T>`, `all`, `race`) are compiler-known, not user-visible machinery.

## Decisions

**1. `async` is a task plus a future, decided at compile time.** `async func f(a, b) -> T` compiles to an ordinary function with the same signature returning `Future<T>`. Its body becomes a closure (the compiler already builds closures that capture by value) that runs as a task, and when the body returns its value the closure completes the future. `return` and `try` inside the body keep their meaning. Alternatives: a state-machine transform (Rust-style) would need an analysis of every suspension point and heap-allocated frames, which fibers make unnecessary; a library-only `spawn` returning a handle would not give `async` syntax or typed futures. `ref` parameters are rejected in `async func` because the closure copies its captures.

**2. `await` parks the current fiber.** `await f` lowers to a call that registers the current task as the future's waiter and switches away; completion moves the waiter back to the ready queue. This is exactly how `channel.receive` already works, so one wake-up mechanism serves both. The "colored function" rule (`await` only in `async func` or `main`) is a compile-time check, cheap to relax later because fibers make `await` safe anywhere that runs inside a task.

**3. Future layout.** A future is a Brasa record in `std/future.br` (`state`, `slot`, `waiters`). The value lives in `slot`, a one-element list created with the element-ownership flag matching `T`, so the existing list destructor releases an unawaited result and no new destructor machinery is needed. `await` takes the value out with a new list intrinsic `take()` that removes the last element without releasing it (ownership moves to the caller). Awaiting twice finds the slot empty and stops with an error. Alternative: keep the value and retain on each await (multi-await); rejected for v1 because the memory model then needs the future to outlive every awaiter.

**4. One scheduler, three sources of wake-ups.** `switch_away()` gains a loop: take a ready task; if none, compute the delay to the earliest timer; if there are no timers and no outstanding I/O, report deadlock; otherwise wait for I/O completions up to that delay, wake tasks whose operations completed or whose timers expired, and retry. To avoid starving timers and I/O when tasks never idle, the scheduler also polls with zero delay every 64 task switches. Alternatives: a dedicated scheduler fiber (extra switches and stack), or an OS timer thread (introduces threads and atomic counts). The idle wait runs on the current fiber.

**5. Timers.** A sorted list of `(deadline, task or future)` using `GetTickCount64`, inserted in order. `sleep(ms)` is an async function whose future completes from the timer list; `timeout(ms, f)` races `f` against a sleep and yields `T or error`. Insertion is O(n) in pending timers; adequate for the intended scale and replaceable by a heap without changing behavior.

**6. I/O through one completion port.** The scheduler owns an I/O completion port created lazily. Each operation allocates an `Op` record whose first bytes are an `OVERLAPPED` (`#[c_layout]`), followed by an operation id. The record is also kept in a table until its completion is processed, so the OS never writes into freed memory even if the awaiting task has gone away. A completion yields the `OVERLAPPED` pointer; the id read at a fixed offset (raw pointer read) finds the operation, stores bytes transferred and status, and wakes its task. Outstanding operations are counted for the deadlock rule.

**7. Files.** `CreateFileW` with `FILE_FLAG_OVERLAPPED`, associated with the port; `ReadFile`/`WriteFile` with explicit offsets in chunks (64 KiB) until end of file or all data written, so large files and partial transfers work and other tasks run between chunks. Paths are converted from UTF-8 with `MultiByteToWideChar`. Handles close in the task that opened them, on every path including errors.

**8. TCP.** Winsock with overlapped sockets: `AcceptEx`, `ConnectEx` (both fetched with `WSAIoctl`, called as `extern` function values), `WSARecv`, `WSASend`. A listener and each connection are records that own their handle. Closing on release needs a language feature the compiler lacks (record destructors only release fields), so decision 11 adds a `drop` hook; `close()` stays available for closing early. Host names go through the blocking `getaddrinfo`; numeric IPv4 addresses are parsed in Brasa and never block.

**9. `all`/`race` are compiler-known.** They are generic over `T`, which the language does not have, so the compiler expands `await all(xs)` into a loop that awaits each future in order (they already run concurrently because async calls start eagerly) and `await race(xs)` into a call to a library function `wait_any` that registers the task on every future's waiter list and returns the index of the first to complete, followed by taking that value. Losing futures keep running and are released when dropped.

**10. Compiler pieces.** Lexer: keywords `async`, `await`. Parser: `async` before `func` on declarations; `await` as a prefix operator at the same level as `try`. AST: an `is_async` flag and `ExprKind::Await`. Types: `Ty::Future(T)` resolved from `Future<T>`; it is a heap type. Native backend only; the bare-metal target rejects the keywords with the same message style used for `spawn`.

**11. A `drop` hook for records.** A record type may declare `func (ref x: T) drop()`; the compiler calls it from that record's generated destructor, before the fields are released, when the last reference goes away. Files, sockets and listeners use it to close their OS handle, which satisfies "released when no longer referenced, including after an error" with no new syntax. Alternative: `defer` (not implemented in the native backend and scoped to a function, so it cannot cover a handle that is returned or stored).

## Implementation notes

Decisions that implementation settled; none changes the specs.

- **Module name.** The library is `std/future.br` (package `future`), because `async` is now a reserved word and cannot name a package. `Future<T>` in user code is compiler-known; inside the library the record is called `Future`.
- **Shared records.** Futures, tasks' operations and OS handles must not be copied on assignment or capture, so records marked `#[shared]` have reference semantics (no copy on assignment, closure capture, iteration or nested copy). `Op`, `Future`, `Buf`, `File`, `Conn` and `Listener` use it. Plain records keep value semantics.
- **Wake-ups are idempotent.** Each task has a `waiting` flag; `wake` only readies a parked task. A stale registration (for instance from `race`, whose losers still list the waiting task) cannot make a task run twice, and every wait is a loop that re-checks its condition.
- **Task ids are recycled.** Finished tasks put their id on a free list after their fiber is deleted, so the task table is bounded by peak concurrency instead of growing with every async call (the leak the first version had).
- **Timers hold closures.** The timer list stores `func()` actions, so the scheduler does not depend on the future library.
- **`timeout` flattens.** `timeout` over a future that already yields `U or error` yields `U or error`, not a nested result. For other futures it yields `T or error`.
- **`timeout`, `all`, `race`, `sleep`** are compiler-known (like `channel`); `timeout` is built by parsing a small Brasa template and reusing the `async` lowering.
- **Winsock declarations** live in `std/ws.br` and shared memory/error helpers in `std/sys.br`, because a Brasa module cannot declare an `extern` named like one of its own functions (`listen`).
- **`extern` function values** are accepted where an integer is expected and the reverse, since they are raw code addresses; this is how `ConnectEx` and `AcceptEx` are called.
- **Native builds are independent.** Each build uses its own work directory, so several can run at once (found when the tests ran in parallel).

## Risks / Trade-offs

- [A task that never reaches `await` starves everyone] → Inherent to cooperative scheduling; documented, and `task.yield()` remains available.
- [Buffers or `OVERLAPPED` freed while the OS still uses them] → The operation table owns each `Op` until its completion is processed; tests exercise abandoned and timed-out operations.
- [Fiber stacks and Winsock/IOCP calls] → Both are plain Win32 calls with no thread affinity beyond the single scheduler thread; verified with a stress test of 100 concurrent connections.
- [Blocking name resolution] → Documented; numeric addresses avoid it; async DNS is a follow-up.
- [Unawaited futures whose task is still running keep their captures alive] → Reference counting releases them when the task finishes; leak tests start and drop futures in a loop.
- [Timer list is O(n)] → Fine for the target scale; the interface hides the structure.
- [Single-await futures are surprising] → A clear runtime error on the second await, and documented.
