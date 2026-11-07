## 1. Language: `shared`, `spawn_thread`, transferable check

- [ ] 1.1 `src/lexer.rs`, `src/parser.rs`, `src/ast.rs`, `brasa.ebnf`: the `shared` expression and type `shared T`, and `spawn_thread` (statement and expression). Verify: new parse tests in `tests/parse.rs` (valid forms, and `spawn_thread` without a call is an error); `tests/docs.rs` still passes with the grammar change.
- [ ] 1.2 A transferability check next to the type checker (in `src/codegen_x64.rs` where types are resolved, or a new `src/transfer.rs`): structural walk with memoization and cycle guard, returning the first offending path and type. Verify: unit tests with records, lists, maps, closures, generics, recursive types and `new T`.
- [ ] 1.3 Apply it to `spawn_thread` arguments, captured variables, and channel element types of channels that cross threads; implement the mutable-global call-graph rule. Verify: `tests/threads.rs` compile-error cases, with messages containing the argument name, the field path `job.owner.next`, the type, and the suggestion (`shared`, `Atomic`, `Mutex`).
- [ ] 1.4 Immutability rule: assignments and mutating operations through `shared T` are errors that name `Mutex<T>`. Verify: compile-error test; read-only use compiles.
- [ ] 1.5 `src/regalloc.rs`: treat `spawn_thread` as `Spawn` (blocks allocation in that statement); `src/lsp.rs`: show the diagnostics and hover for `shared`. Verify: `tests/optimizer.rs`, `tests/lsp.rs` pass; a small LSP test for the new error.

## 2. Runtime: atomics, allocator, thread block

- [ ] 2.1 `src/codegen_x64.rs` (`RUNTIME_COMMON`): `rt_retain_atomic`/`rt_release_atomic` with the same header layout; the compiler selects them from the static type of a `shared` value. Verify: `--asm` of a `shared` program contains `lock`; `--asm` of a program with no `shared`/`spawn_thread` has no `lock` (test in `tests/threads.rs`).
- [ ] 2.2 The atomic flag (`rt_make_atomic`) for copy-on-write values that enter a `shared`, a cross-thread channel or `spawn_thread` (coordinate with add-copy-on-write's write check); omitted entirely when the program uses no threads. Verify: a two-thread test where both threads append to the same received list and the original is unchanged.
- [ ] 2.3 Thread block (TLS): scheduler pointer, allocator free lists and bump chunk, live counter, thread id; the registry of live blocks. Windows: TLS slot; Linux: `CLONE_SETTLS` base. Verify: `tests/threads.rs` allocator stress (8 threads x 10000 alloc/free) leaves `live_objects()` unchanged and does not crash; run under WSL for Linux.
- [ ] 2.4 Thread-safe allocator on all three runtime variants (Windows, Linux, bare); the bare target keeps the single-thread allocator. Verify: existing `tests/codegen.rs`, `tests/linux.rs`, `tests/os.rs` pass unchanged; the stress test above.
- [ ] 2.5 `live_objects()` as the sum over thread blocks plus exited threads. Verify: exact after join in the test of the spec; unchanged for single-thread programs.

## 3. Libraries

- [ ] 3.1 New `std/thread.br` (Windows: `CreateThread`, `WaitForSingleObject`, per-thread entry stub) and `std/thread.linux.br` (`mmap` stack with guard, `clone`, `futex` join): `start`, `join`, `id`, `sleep`. Verify: run/join and parallel-speedup tests; a stress test of 200 short threads; run under WSL.
- [ ] 3.2 New `std/sync.br` with `Mutex<T>` (futex or `WaitOnAddress`, owner check, closure-style `with` API) and `Atomic`. Verify: counter-contention tests (80000 and 400000), re-entrant lock error, early return with `defer`.
- [ ] 3.3 `std/task.br` and `std/task.linux.br`: move scheduler variables into a per-thread `Sched` record reached through the thread block; per-thread I/O port or `epoll`+`eventfd`; the inbox, `wake_remote`, per-thread deadlock detection with a live-thread count. Verify: the whole of `tests/async.rs` (run separately, as per the project routine) and `tests/udp.rs`, `dns.rs`, `tls.rs` pass unchanged; new tests for wakeup-while-sleeping and per-thread deadlock.
- [ ] 3.4 `SChan` in `std/task*.br` with a lock word and remote waiters, selected by the compiler for channels that cross threads. Verify: four producers and one consumer exactly-once test; closing wakes all receivers; an in-thread channel's generated code is unchanged.
- [ ] 3.5 `std/future.br`, `std/net.br`, `std/net.linux.br`, `std/fs.br`, `std/fs.linux.br`, `std/tls*.br`, `std/ui*.br` (UI stays main-thread only, documented): read per-thread state instead of globals. Verify: their existing suites pass; an async-on-thread test.
- [ ] 3.6 `os/` kernel: `spawn_thread` rejected with a message; `shared` falls back to plain counts. Verify: `tests/os.rs` and a test for the rejection message.

## 4. Examples, docs, tests

- [ ] 4.1 `examples/`: a parallel-sum example (shared list, worker threads, channel of results) and a mutex counter example, deterministic output. Verify: they build and print the same on Windows and, under WSL, Linux (`tests/threads.rs`, `tests/linux.rs`).
- [ ] 4.2 `tests/threads.rs`: one test per spec scenario (both capabilities), Linux cases skipped without WSL. Verify: `cargo test --test threads`.
- [ ] 4.3 Docs: `docs/concurrency.html` (threads, `shared`, `Mutex`, per-thread schedulers, the transferable rule and its errors), `docs/native.html` (table row for threads on both targets), `docs/index.html` and `docs/getting-started.html` where concurrency is mentioned, README (replace "run on one thread", add the roadmap row for the stage 2 work-stealing scheduler). Verify: `cargo test --test docs`.
- [ ] 4.4 Run the full suite except `tests/async.rs` (project routine), then `tests/async.rs` once because the scheduler changed, and `openspec validate add-threads-and-shared --strict`.
