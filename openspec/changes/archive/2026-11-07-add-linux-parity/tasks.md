## 1. Foundations

- [x] 1.1 Division and remainder by zero: a check before `idiv` that panics with `division by zero` (Windows and Linux; not the kernel); tests on both targets and the optimizer suite
- [x] 1.2 Library selection by target: `std/<name>.linux.br` before `std/<name>.br`; the gating list shrinks to `ui` and `ws`; a test that every `.linux.br` file exports the same names and signatures as its Windows counterpart
- [x] 1.3 `std/sys.linux.br`: the `errno` message table, `Unknown error N`, the `-errno` convention for system call results

## 2. Tasks and time

- [x] 2.1 Runtime: stack allocation (`mmap` with a guard page, cached after use), the context switch and the new-fiber trampoline
- [x] 2.2 `std/task.linux.br`: `now`, timers, the fiber operations, `poll_io` over `epoll` (create, add, wait), deadlock detection; the shared channel, `park`/`wake` and `future` code unchanged
- [x] 2.3 Tests: ordering by sleep time, yield, channels, `async`/`await`, deadlock error, ten thousand tasks, the example programs that use tasks compared with Windows

## 3. Files

- [x] 3.1 `std/fs.linux.br`: `open`, `read`, `write`, `pread`/`pwrite`, `fstat` size, `close`, the directory operations `fs` has on Windows; chunks of 64 KiB with a `yield`; errors as `path: reason`
- [x] 3.2 Tests: round trip of 1 MiB, missing file, `bufio` over a file, files while other tasks run

## 4. Sockets and names

- [x] 4.1 `std/net.linux.br` TCP: `listen`, `accept4`, `connect`, `read`, `write`, `close` on non-blocking sockets driven by `epoll`; the `Reader`/`Writer` code shared with Windows
- [x] 4.2 UDP: `udp`, `send_to`, `recv`, truncation reported as on Windows
- [x] 4.3 Name lookup: `/etc/hosts`, literals, `/etc/resolv.conf`, DNS query and answer parsing (compression, `CNAME`), timeout and retry, TCP fallback; `lookup`, `resolve` and their `await` forms
- [x] 4.4 Tests: echo, UDP exchange, large transfers, peer close, `localhost`, literals, an `.invalid` name, a real name when a resolver answers (skipped otherwise); the existing `net`, `udp` and `dns` suites run on both targets

## 5. Calling C

- [x] 5.1 The dynamic ELF writer: interpreter, `DT_NEEDED` from `#[link]`, symbol and string tables, hash, relocations into a pointer table, `DT_BIND_NOW`; static output unchanged for programs without externs
- [x] 5.2 System V thunks for `extern` calls and callbacks; library name mapping; `#[link("c")]` and `#[link("m")]`
- [x] 5.3 Tests: `getpid`, `strlen`, a `math` function, a callback (`qsort`), static programs have no interpreter; document the libc start-up limitation after checking glibc and musl under WSL

## 6. Documentation and verification

- [x] 6.1 `docs/native.html`: the compatibility table (every library and feature, Windows and Linux, notes on `ui` and on file I/O being synchronous per chunk); `docs/async.html` and `docs/stdlib.html` where they say Windows only; README rows and roadmap
- [x] 6.2 Cross-platform programs folder and a test that builds and compares each one on both targets
- [x] 6.3 Prove nothing else moved: boot image byte-identical, Windows suites green, benchmark of the division check, `--fmt-check`, `openspec validate add-linux-parity`, the full suite including `async`
