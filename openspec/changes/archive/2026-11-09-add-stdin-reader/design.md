## Context

`fs.Reader` (`std/fs.zila`, `std/fs.linux.zila`) wraps a `File` and a `bufio.InBuf`; `fill()` calls `transfer`, which uses overlapped `ReadFile` with an offset on Windows and `pread` on Linux. Both need a seekable, completion-port-attached file, which standard input usually is not (pipes and consoles have no offset). The two layers must keep the same public surface (`tests/std_parity.rs`).

## Goals / Non-Goals

**Goals:** the same `Reader` API for standard input; both platforms; pipes, files and terminals; end of input like end of file.

**Non-Goals:** not blocking other tasks while waiting; terminal modes; a writer for standard output (see proposal.md).

## Decisions

**Reuse `Reader`, add a flag.** `Reader` gets `pub std: bool`; `fill()` takes a different path when it is set and everything else (`read_line`, `read`, `read_all`, `at_end`, buffering) is shared. Alternative: a separate `Stdin` type with copies of the methods. Rejected: duplicate code in two layers for no behavioural difference.

**Synchronous read.** Windows: `GetStdHandle(-10)` and a plain `ReadFile` with no `OVERLAPPED`; a broken pipe (`ERROR_BROKEN_PIPE`, 109) at the writer's end is the end of input. Linux: `read(0, ...)` (system call 0), retried after `EINTR`. Alternative: attach the handle to the completion port, or read on a helper thread so other tasks keep running. Rejected for now: a standard input handle is not opened for overlapped I/O, and the helper thread needs the Linux thread layer, which does not exist (see the `threads` spec). The blocking behaviour is documented; the API does not change if it is made asynchronous later, since the methods are already `async`.

**Never owns the handle.** The `File` inside the reader has `open: false`, so `close()` and `drop` do nothing to the process's standard input.

**Reads in 64 KiB pieces**, the size `fs` already uses, so a large redirected file is not read byte by byte.

## Risks / Trade-offs

- [A task waiting on standard input freezes timers and other tasks] → documented; programs that need both read input in a thread once the Linux thread layer exists.
- [Console input on Windows arrives in the console code page] → text is passed through as bytes like any file; documented with the note that pipes and files are exact.
- [`Reader` gains a field, so code that builds a `Reader` by hand breaks] → only `fs.open` builds one in this repository; `net` has its own `Reader` type.
