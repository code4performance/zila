## Why

`--target linux` (change `add-linux-target`) runs the language and the pure libraries, but a Zila Lang program that reads a file, opens a socket, sleeps, spawns a task or calls `async` still builds only for Windows. For the language to do the same on both platforms, every library and feature that is rejected on Linux today needs a Linux implementation behind the same API, so that one source file builds for either target.

What is missing on Linux today:

| Missing | Why it fails today | What Linux uses instead |
|---|---|---|
| `task`, `spawn`, `sleep`, channels, `async`/`await`, `future` | fibers, timers and the I/O completion port are Windows calls (`CreateFiber`, `SwitchToFiber`, `GetQueuedCompletionStatusEx`) | a context switch written in assembly, stacks from `mmap`, `epoll` and `clock_gettime` |
| `fs` | `CreateFileW`, `ReadFile`, `WriteFile`, overlapped I/O | `open`, `read`, `write`, `pread`, `pwrite`, `fstat`, `close`, `unlink`, `rename`, `mkdir` |
| `net` (TCP, UDP) | Winsock (`ws`) and the completion port | `socket`, `bind`, `listen`, `accept4`, `connect`, `sendto`, `recvfrom`, non-blocking with `epoll` |
| `net` name lookup (`lookup`, `resolve`) | `GetAddrInfoExW` | a DNS client written in Zila Lang over UDP (`/etc/hosts`, `/etc/resolv.conf`), since there is no libc |
| `sys` | `FormatMessageA`, Windows error codes | an `errno` message table |
| `extern` / `#[link]` | imports from DLLs | a dynamically linked ELF that calls C with the System V convention |
| division by zero | neither target prints a message: Windows ends with a hardware exception, Linux with a signal (status 136) | a check in the generated code on both targets: `panic: division by zero`, status 101 |
| `ui` | Win32 windows | **not planned here**: a window toolkit is a separate decision (see design) |

Smaller differences: the small-object heap is capped at 256 MiB, and the Linux tests need WSL.

## What Changes

- Every library in the table above gets a Linux implementation with the **same public API and the same observable behavior** as on Windows; a program using it builds with `--target linux` and prints what it prints on Windows. The "not available on Linux yet" rejection is removed for each library as it lands, and stays for `ui` (and `ws`, which is an internal Windows binding).
- Libraries are chosen by target: when the target is Linux, `use fs` loads `std/fs.linux.br` if it exists, otherwise `std/fs.br`. The platform-neutral parts (channels, futures, `bufio`, `format`) stay in the shared files; only the layer that touches the operating system is duplicated.
- The runtime gains fibers (`rt_fiber_*`), a time source, `epoll` and the stub resolver; integer `/` and `%` get a zero check that panics on both hosted targets (the optimizer drops it for constant non-zero divisors).
- `extern` functions and `#[link("name")]` work on Linux with the System V calling convention: the compiler writes a dynamically linked executable (interpreter, `DT_NEEDED`, a table of imports filled by the loader) when a program declares externs, and keeps the static executable otherwise.
- `tests/linux.rs` and the existing async, net, fs, udp and dns suites run a shared set of programs on both targets and compare the output.
- Documentation: a compatibility table (what each library does on each platform) in `docs/native.html`, and the README row.

## Capabilities

### New Capabilities

- `linux-parity`: the libraries and features that give a program the same behavior on Linux as on Windows.

### Modified Capabilities

- `linux-target`: the requirement that rejects `fs`, `net`, `task`, `future`, `sys` and `extern` on Linux is replaced by the rule that only `ui` and `ws` are rejected (it is in the change `add-linux-target` until that is archived).

## Impact

- `src/codegen_x64.rs`: library selection by target, Linux runtime additions, the dynamic ELF writer, the division check; `src/asm_x86.rs`: any instructions the new assembly needs (`rdtsc` is not needed; `xchg`, `cmpxchg` are not either; `syscall` already exists).
- New `std/task.linux.br`, `std/fs.linux.br`, `std/net.linux.br`, `std/sys.linux.br` (and `std/ws` untouched); small changes to `std/future.br` if it names Windows types.
- New and extended tests, docs, README.
- The boot image must stay byte-for-byte the same; the Windows build changes only by the division check.
