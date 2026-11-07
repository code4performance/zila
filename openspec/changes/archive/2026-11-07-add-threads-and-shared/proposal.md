## Why

Brasa tasks are cooperative and run on one thread (README, `std/task.br`), and every reference count is a plain, non-atomic `inc`/`dec` (`rt_retain`/`rt_release` in `src/codegen_x64.rs`). A program cannot use a second core, and a blocking system call (a long computation, a blocking file read) freezes every task. The project owner chose real multi-threading in the style of Rust's `Rc`/`Arc`: `new T` (add-weak-refs) stays cheap and non-atomic, and a second reference kind, `shared T`, has atomic counts and may cross threads. The compiler decides what may cross a thread boundary by looking at the types, with no annotation from the user.

## What Changes

- **New reference kind `shared T`**: `shared expr` allocates a heap object whose reference count is atomic. It is immutable by default; mutation goes through an explicit lock type `Mutex<T>` (or an atomic cell `Atomic` for one integer). `shared` is the only way a heap object is visible to more than one thread.
- **Transferable types**: the compiler computes structurally whether a type is *transferable*: numbers, bools, text and enums are; records, lists, maps, sets, tuples and closures are if all fields, elements and captures are; `shared T` and `Mutex<T>` are if `T` is; `Atomic` is; `new T` is not. Nothing is annotated.
- **`spawn_thread f(args)`**: starts `f` on a new OS thread (Windows `CreateThread`, Linux `clone` through the compiler's own system calls, no libc). Arguments and captures must be transferable; otherwise a compile error names the argument and the field path that is not (for example `argument 'job': field 'job.owner' has type 'new Node', which is not transferable`). It returns a `Thread` handle with `join()`.
- **Thread-safe channel**: a channel used across threads requires a transferable element type and is safe for any number of sending and receiving threads; a cross-thread `send` wakes a parked receiver on its own thread.
- **Per-thread scheduler**: each thread has its own cooperative scheduler, run queue, timers and I/O wait set; tasks never migrate between threads. Cross-thread wakeups go through a per-thread atomic inbox. `live_objects()` becomes a thread-safe global count.
- **Mutable global `var`** cannot be touched by a thread-started function unless its type is `shared` or `Mutex`; the compiler rejects it.
- **Runtime**: the allocator becomes thread safe (per-thread free lists with a locked fallback), `live_objects` counting is atomic, and reference-count operations on a `shared` object use `lock` instructions. Copy-on-write (add-copy-on-write) uses the atomic path for any value reachable through a `shared` reference, a cross-thread channel or `spawn_thread`.
- **BREAKING (pre-1.0)**: the README sentence "Tasks are cooperative and run on one thread" is replaced, and a global `var` used by a `spawn_thread` function stops compiling. Existing single-thread programs behave the same and pay no atomic cost.
- **Staged**: stage 1 (this change) is `shared`, the transferable rule, `spawn_thread`, the thread-safe channel and `Mutex`. A work-stealing parallel scheduler that moves tasks between threads is out of scope and left for a later change.

## Capabilities

### New Capabilities

- `threads`: OS threads, `spawn_thread`, join, the transferable rule and its errors, thread-safe channels, per-thread scheduling, blocking behavior, `live_objects()` under threads.
- `shared-references`: `shared T`, atomic counts, immutability, `Mutex<T>` and `Atomic`, identity and weak behavior, interaction with copy-on-write.

### Modified Capabilities

None. The wording "one scheduler" in `async-execution` and `docs/concurrency.html` is reconciled by the tasks below; `remove-async-coloring` rewrites that spec, so no delta is written here.

## Impact

- `src/lexer.rs`, `src/parser.rs`, `src/ast.rs` (`shared`, `spawn_thread`), a new transferability check next to the type checker, `src/codegen_x64.rs` (atomic `rt_retain_atomic`/`rt_release_atomic`, thread-safe allocator, thread entry and thread-local scheduler state, `live_objects`), `src/regalloc.rs` (treat `spawn_thread` like `Spawn`), `src/lsp.rs` (diagnostics, hover).
- `std/task.br`, `std/task.linux.br` (per-thread scheduler state, cross-thread wake), new `std/thread.br`, `std/thread.linux.br`, `std/sync.br` (`Mutex`, `Atomic`), `std/future.br`, `std/net.br`, `std/net.linux.br`, `std/fs.br`, `std/fs.linux.br` (per-thread I/O state), `os/` (kernel target rejects threads), `examples/`, `docs/concurrency.html`, `docs/native.html`, README, `brasa.ebnf`.
- Tests: new `tests/threads.rs`; existing suites must pass (`tests/async.rs` is run separately, per the project's routine).
