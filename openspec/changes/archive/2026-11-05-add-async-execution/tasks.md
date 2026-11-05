## 1. Front end

- [x] 1.1 Add the keywords `async` and `await` to the lexer and update the keyword lists in `brasa.ebnf` and the editor grammar; verify with a lexer test that both lex as keywords and that existing examples still parse
- [x] 1.2 Parse `async func` declarations (flag on `FuncDecl`) and `await expr` as a prefix operator at the level of `try`, with `Future<T>` accepted as a type; verify with parser tests for an async function, `await f()`, `try await f()` and `await` in a lambda argument
- [x] 1.3 Reject `async` on methods with `ref` parameters, on `extern` functions and on lambdas, and reject `await` outside an `async func` or `main`; verify each with a compile-error test whose message names the rule

## 2. Futures and the scheduler

- [x] 2.1 Add the list intrinsic `take()` (remove the last element without releasing it) to the runtime and compiler; verify with a program that takes a text from a list and `live_objects()` shows no double release or leak
- [x] 2.2 Create `std/future.br` with the `Future` record (`state`, `slot`, `waiters`), `future_new`, `complete`, `wait`, `wait_any`; verify with a program that completes a future from a task and awaits it from `main`
- [x] 2.3 Add `Ty::Future(T)` to the native backend (resolution of `Future<T>`, heap type, compatibility) and the compile-time `await` context check; verify `let x: int = f()` for an async `f` is a type error and an unawaited future is released
- [x] 2.4 Lower `async func` to a wrapper that builds the future, starts the body as a task and completes the future with its return value, including `or error` results; verify the call returns before the body finishes, `await` yields the value, and `try await` propagates an error
- [x] 2.5 Lower `await e` to `wait` with the value taken from the slot and a clear error on a second await; verify awaiting twice stops with the documented message
- [x] 2.6 Extend `std/task.br`: timer list, outstanding-operation counter, idle wait loop in `switch_away()`, zero-delay poll every 64 switches, and the new deadlock rule; verify the existing `spawn`/`channel` tests still pass and a program whose only task waits on a timer does not report deadlock while a truly stuck program still does

## 3. Timers and combinators

- [x] 3.1 Implement `sleep(ms)` in `std/future.br` with `GetTickCount64` deadlines; verify sleep durations are at least the request, timers fire in deadline order, and three concurrent 100 ms sleeps finish in about 100 ms
- [x] 3.2 Implement `timeout(ms, future)` returning `T or error`; verify it yields the value in time and a timed-out error otherwise, and that the abandoned task still finishes without leaks
- [x] 3.3 Implement `await all(xs)` and `await race(xs)` as compiler expansions; verify `all` keeps order, `race` returns the fastest after about its delay, and the losing futures are released

## 4. Files

- [x] 4.0 Add the record `drop` hook to the native backend (the generated destructor calls `drop` before releasing fields); verify with a program whose record prints in `drop` when its last reference is released, including on early `return` and after `try` failures, and that records without `drop` are unchanged

- [x] 4.1 Add `c.from_bytes(ptr, len)` and text-to-buffer helpers to the `c` intrinsics; verify with a round trip of a text containing multi-byte characters
- [x] 4.2 Create `std/fs.br` with the I/O completion port setup, `Op` records and the completion dispatch in the scheduler; verify with a debug build that a completed operation wakes exactly its task and the outstanding counter returns to zero
- [x] 4.3 Implement `fs.read` and `fs.write` (chunked, overlapped, UTF-8 paths, errors naming the path and reason); verify round trip, missing file, a multi-megabyte file with identical contents, and another task printing during a large read
- [x] 4.4 Verify no handle or object leaks by reading and writing 1000 files in a loop and checking `live_objects()` and that the process can still open files

## 5. Network

- [x] 5.1 Create `std/net.br` with Winsock startup, overlapped sockets, `AcceptEx`/`ConnectEx` pointers via `WSAIoctl`, and numeric IPv4 parsing; verify connecting to a local listener works
- [x] 5.2 Implement `net.listen`, `accept`, `connect`, `read`, `write` and `close` with descriptive errors and blocking `getaddrinfo` only for host names; verify the loopback echo, connection refused, and peer-closed behaviors
- [x] 5.3 Stress and release test: 100 simultaneous clients each exchanging a message, then 1000 sequential connect/close cycles; verify every reply is correct, no handles are leaked and `live_objects()` stays bounded

## 6. Platform rules and hardening

- [x] 6.1 Make the bare-metal target reject `async`, `await` and the new libraries with a clear message; verify with a `tests/os.rs` style test
- [x] 6.2 Add a leak test that starts and awaits 10000 async calls in a loop; verify the live object count after the loop is within a small constant of before it
- [x] 6.3 Add an end-to-end test combining sleeps, a file, a TCP echo, a channel and `timeout` in one program; verify its output and that it exits with code 0

## 7. Documentation and examples

- [x] 7.1 Add `examples/async.br` (concurrent sleeps, a file round trip and a TCP echo with `try await`) and make `tests/` build and run it; verify its output
- [x] 7.2 Document `async`/`await`, `Future<T>`, `sleep`, `timeout`, `all`/`race`, `fs` and `net` in `docs/` (a new page linked from the shared navigation, plus updates to the syntax page and getting-started) with their limits (single thread, no cancellation, single await, blocking name lookup); verify every code sample compiles and every nav link works from disk
- [x] 7.3 Update `README.md` and `brasa.ebnf` for the new syntax and libraries; verify `cargo test` passes and no page still says `spawn` is the only concurrency primitive
