## Context

`std/fs.br` has an internal `File` (shared record owning a Windows handle), `open_file(path, write)` and `transfer(f, ptr, count, offset, write, path)`, which does one overlapped read or write at an explicit offset and awaits it; the public `read` and `write` move whole files in 64 KiB pieces. `std/net.br` has `Conn` with `read(max)` and `write(data)` (one `WSARecv` / `WSASend` each). Text is a byte string with `len`, `slice(a, b)`, `+` and `c.cstr`/`c.from_bytes` for raw memory; `sys.buffer(n)` allocates raw memory.

## Goals / Non-Goals

**Goals**
- Line reading and buffered writing for files and sockets with one shared, testable implementation of the buffering rules.
- About one system call per 64 KiB.

**Non-Goals**
- Seeking, reading and writing the same file through one object, memory-mapped files, directories, UDP, TLS, a separate text/UTF-8 decoding layer.
- Flushing from a destructor.

## Decisions

1. **A pure core, `std/bufio.br`.** `InBuf` (`feed`, `finish`, `has_line`, `take_line`, `take`, `take_all`, `is_empty`) holds a `data: text`, a read position `pos`, and `done: bool`; `OutBuf` (`add`, `len`, `take`) accumulates pieces. Both are `#[shared]` records. `feed` appends the new chunk to the unread tail (`data.slice(pos, len) + chunk`), so the buffer does not grow with what was already consumed; the tail is only what the consumer has not taken. `has_line` looks for `\n` in the unread data (scanning from where the last scan stopped, so a very long line is scanned once); `take_line` returns the text before it without a trailing `\r`. After `finish()` the unread rest counts as one last line. The core uses no system call, so tests also run it in the interpreter.
2. **Readers.** `fs.Reader` holds the `File`, the path, the next file offset, an `InBuf`, and a `reads` counter. A private async `fill()` calls `transfer` for 64 KiB at the offset, advances the offset, `feed`s the bytes and calls `finish()` when the transfer returned 0. `read_line` loops `while not buf.has_line() and not buf.done`: `try await fill()`; then returns `take_line()` or the error `end of file`. `read(max)` returns buffered bytes first, otherwise fills once; `read_all` fills until done. `at_end()` fills when the buffer is empty and not done. `net.Reader` is the same with a `Conn`: `fill` awaits `conn.read(65536)` and an empty result calls `finish()`. The two reader types are written separately (they differ only in `fill`) around the shared `InBuf`, because async methods cannot be passed around as values yet.
3. **Writers.** `fs.Writer` holds the `File`, the path, the next offset (the file size for `append`), an `OutBuf`, and a `writes` counter. `write` adds the text and, when the buffer holds at least 64 KiB, calls the private `drain()` that hands the whole buffer to one `transfer` (more only if the system wrote part of it), so `writes` stays near the data size divided by 64 KiB. `flush` drains, `close` drains and releases the handle. `net.Writer` does the same with `conn.write` (which already loops until all bytes are sent) in one call per drain; its `close()` only flushes, because the connection may still be in use by a reader.
4. **Open modes.** `fs.open` reuses `open_file(path, false)`; `fs.create` is `open_file(path, true)` (create or replace); `fs.append` needs a third mode (`OPEN_ALWAYS`, disposition 4) and the file size as the first offset (`file_size`).
5. **End of data.** The error for the end of a reader is `error("end of file")`; programs compare the message, or use `at_end()` first. `read(max)` and `read_all` never fail at the end: they return empty text.
6. **Drop.** The readers and writers own shared records; the file handle is closed by `File.drop` when the last reference goes away, and a writer simply discards its buffer when dropped. The docs say to `close()` writers.
7. **Registration.** `bufio` is added to `std_source` and the interpreter loader like the other pure libraries; `fs` and `net` `use bufio`.
8. **Tests.** Native tests for lines of every ending, empty file, long and multibyte lines, mixed calls, the read and write counters on a file of 100,000 lines, append, flush behaviour, a socket echo of lines split across packets, and leaks. The `bufio` scenarios run natively and in the interpreter.

## Risks / Trade-offs

- Appending to an `InBuf` copies the unread tail on every `feed`; with 64 KiB pieces and lines that are short compared to that, the copy is small. A single line much larger than the chunk is copied once per chunk (quadratic in the number of chunks it spans); the 200,000-character line test bounds the cost.
- Two reader types and two writer types duplicate a few lines each; the shared buffers keep the real logic in one place.
- A writer dropped without `close()` loses data: documented, and `close()` is the normal path.
