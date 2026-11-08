## Why

Zila Lang has `spawn` and `channel`, but a task can only wait for another task or a channel. It cannot wait for time, a file or the network, so a program that sleeps, reads a file or talks over TCP either blocks every other task or cannot be written at all. Programs that serve or fetch things (the REST example, a chat server, the shell of an OS) need to overlap waiting with work. An `async`/`await` model gives beginners a single readable way to write that: a function that waits is marked `async`, and `await` is the only place a task gives up the processor.

## What Changes

- **Language**: new keywords `async` and `await`.
  - `async func f(...) -> T` starts running when called and returns a `Future<T>` immediately.
  - `await e` waits for a future without blocking other tasks and yields its value; it composes with errors (`try await f()` for `-> T or error`).
  - `await` is allowed only inside an `async func` and inside `main`.
  - `Future<T>` is a new type usable in signatures, variables and lists.
- **Scheduler**: the task scheduler (`std/task.br`) learns to wait for timers and I/O completions instead of declaring deadlock when nothing is ready. It reports deadlock only when no task is ready, no timer is pending and no I/O is outstanding.
- **Timers**: `await sleep(ms)` and `await timeout(ms, future)`; a timer never blocks other tasks.
- **Combinators**: `all(futures)` waits for every future and `race(futures)` for the first one.
- **Files** (new `fs` library): `await fs.read(path)`, `await fs.write(path, data)`, using overlapped Windows I/O.
- **Network** (new `net` library): TCP `connect`, `listen`, `accept`, and `read`/`write`/`close` on a connection, all awaitable, using Winsock with an I/O completion port.
- **Native backend only**, Windows x64. The bare-metal target rejects `async`, as it does `spawn`.
- Existing `spawn`, `channel`, `task.yield()` keep working and share the same scheduler.
- Documentation and an `examples/async.br` program are added in the same change.

## Capabilities

### New Capabilities
- `async-execution`: `async func`, `Future<T>`, `await`, `all`/`race`, and the rules for how tasks run, wait, finish and deadlock on the shared scheduler.
- `async-io`: awaitable timers, file reads and writes, and TCP networking, with their error behavior and limits.

### Modified Capabilities
<!-- None: there are no main specs yet; concurrency (`spawn`/`channel`) behavior is unchanged. -->

## Impact

- **Compiler** (`src/lexer.rs`, `parser.rs`, `ast.rs`, `codegen_x64.rs`): two keywords, an `async` flag on function declarations, an `Await` expression, the `Future<T>` type, lowering of `async func` to a task plus a future, new intrinsics, and import of the new libraries.
- **Standard library** (written in Zila Lang, like `ui` and `task`): extended `std/task.br`, new `std/async.br`, `std/fs.br`, `std/net.br`.
- **Runtime**: no new assembly beyond what `task.br` needs; I/O uses Windows `kernel32` and `ws2_32` through `extern`.
- **Grammar, tests, docs**: `zila.ebnf`, editor grammar, `tests/`, `docs/` and `README.md`.
- **Assumptions** (no clarification needed):
  - Tasks stay cooperative on one thread (no parallelism); switches happen only at `await`, `receive` and `task.yield()`.
  - Futures are single-consumer: a future's value is taken by the first `await`.
  - There is no cancellation in this version: a task whose future timed out keeps running to completion.
  - Host-name resolution uses the blocking Windows resolver and therefore blocks the scheduler briefly; numeric addresses do not.
  - Not included: TLS, UDP, async DNS, directory operations, and integration with the `ui` message loop.
