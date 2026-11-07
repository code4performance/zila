## Context

The scheduler (`std/task.br`) runs fibers on one thread. An operation is an `Op` record whose first fields are a Windows OVERLAPPED followed by an `id`; `op_submit` counts it as pending and parks nothing by itself, `op_wait` parks the task until `op.done`, and `poll_io(ms)` waits on the I/O completion port with `GetQueuedCompletionStatus` (not alertable), reads the `id` from the OVERLAPPED (offset 32), fills in `status`, `bytes` and `done`, and wakes the task. `net.resolve(host)` parses numeric addresses and otherwise calls `getaddrinfo` (blocking). Brasa functions can be passed to the system as `extern "system" func` callbacks (the scheduler passes `fiber_main` to `CreateFiber`).

`GetAddrInfoExW(name, service, namespace, provider, hints, &result, timeout, overlapped, completion_routine, &cancel_handle)` resolves in a system worker thread. With an OVERLAPPED and a completion routine, it returns `WSA_IO_PENDING` (997) and later queues the routine as an APC to the thread that called it; the routine runs only when that thread is in an alertable wait. When it fails or succeeds at once it returns that code and does not call the routine. A completion on the I/O completion port is not an option for this API.

## Goals / Non-Goals

**Goals**
- No call in `net` blocks the other tasks, DNS included, without threads and without touching the memory model of the runtime.

**Non-Goals**
- Cancelling a lookup (a lookup abandoned by `timeout` finishes in the background and its result is dropped), IPv6, internationalized names, caching, other record types (MX, SRV...), a pure-Brasa resolver.

## Decisions

1. **Alertable wait.** `poll_io` calls `GetQueuedCompletionStatusEx(port, entries, 1, &removed, ms, alertable = TRUE)` instead of `GetQueuedCompletionStatus`. It returns a completed I/O as before (`entries[0]` is an `OVERLAPPED_ENTRY`: key, overlapped pointer, internal, bytes), or returns `FALSE` with `WAIT_IO_COMPLETION` (192) after running a queued routine (then the loop continues without blocking, as after any completion), or `FALSE` with `WAIT_TIMEOUT` (258) when nothing happened. Waiting with `Sleep` when only timers are pending is unchanged (a lookup counts as a pending operation, so a wait with a lookup in flight goes through `poll_io`).
2. **The completion routine.** `dns_done(error: u32, bytes: u32, overlapped: usize)` is a Brasa function passed to `GetAddrInfoExW`. It reads the op id from the OVERLAPPED (as `poll_io` does), sets `op.status = error`, `op.done = true`, decrements the pending count and wakes the task. It does no allocation and never switches fibers. It runs only inside the alertable wait of `poll_io`, where the scheduler's own state is consistent, so it may use the scheduler's data structures like `poll_io` itself does.
3. **`Op` reuse.** A lookup uses an `Op` like any I/O (`op_new`, `op_submit`, `op_wait`, `op_release`). Ops are recycled, never freed, so a late completion for an abandoned lookup (after `timeout`) still finds a valid record; it marks it done and wakes no one if its task has moved on (`task = 0` after release is not enough, so `dns_done` checks that the op is still the one the waiting task submitted, using a generation number stored in `op.bytes` before the call, or simply leaves a finished-with-no-waiter op to be recycled).
4. **`lookup`.** `async func lookup(host: text) -> text or error`: a numeric address is returned as parsed; otherwise `net_init()`, `task.io_init()`, convert the name to UTF-16 (`sys.wide`, moved from `fs`), build `ADDRINFOEXW` hints (family `AF_INET`, socket type stream), allocate the result pointer, call `GetAddrInfoExW(name, 0, NS_DNS = 12, 0, hints, &result, 0, &op, dns_done, 0)`. If it returns 0 the answer is already in `result`; if it returns 997 the task waits with `task.op_submit` / `op_wait` and then reads the status; any other code is an error. The first `AF_INET` entry's address (`ai_addr` at offset 32, `ADDRINFOEXW.ai_next` at offset 64) becomes `a.b.c.d`, and `FreeAddrInfoExW` frees the list. Any error message is `cannot resolve '<host>'`.
5. **Users of `resolve`.** `resolve(host) -> int or error` becomes `async` (it parses the text from `lookup` back into the number `make_addr` needs); `connect` and `send_to` await it. The numeric-address path never reaches the resolver.
6. **Tests.** Native tests on `localhost` and malformed names only (no outside network): lookups of `localhost`, a numeric address, a malformed name, concurrent lookups with a ticker, `connect` and `send_to` by name, `timeout` around `lookup`, and a loop for leaks. The whole async, files, TCP, UDP and buffered I/O suites are re-run as the regression for the scheduler change.

## Risks / Trade-offs

- Running Brasa code as an APC inside the scheduler is new; the safeguards are that it only runs in one known call, touches no heap values and the full asynchronous test suites run against it.
- A timed-out lookup keeps a resolver request and its result memory until it finishes; the result is freed by the routine path so nothing leaks, only lingers.
- `localhost` may complete synchronously (no APC), which is why the synchronous return code is handled and why the concurrency test uses several names in flight to exercise the pending path as much as the machine allows.
