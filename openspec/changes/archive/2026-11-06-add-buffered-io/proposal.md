## Why

Files can only be read or written whole (`fs.read`, `fs.write`), and a connection hands back whatever the network delivered (`conn.read(max)`), so a program that wants "the next line" or "write a few thousand small pieces" has to do its own buffering, text splitting and bookkeeping, or pay one system call per piece. Line-oriented protocols (HTTP, logs, CSV, chat) and large files are the everyday cases. Buffered I/O is the next item of the standard library.

## What Changes

- New `fs.open(path)` giving an `fs.Reader`, and `fs.create(path)` / `fs.append(path)` giving an `fs.Writer`. Nothing is read or written for the file as a whole: data moves in 64 KiB pieces.
- Reader methods (all awaitable, none blocks other tasks): `read_line()` (the next line without its `\n` or `\r\n`; the last line may have no terminator; an error saying `end of file` when there is none), `read(max)` (up to `max` bytes, empty text at the end), `read_all()` (the rest), `at_end()` (true when nothing is left, which may wait for data) and `close()`.
- Writer methods: `write(text)` and `write_line(text)` collect data in a buffer and hand it to the system only when 64 KiB have gathered, on `flush()` or on `close()`.
- `conn.reader()` and `conn.writer()` give the same `Reader` and `Writer` types over a TCP connection, so `read_line()` works on a socket and many small `write` calls become few sends. (`conn.read` and `conn.write` stay as they are.)
- The buffering and line splitting live in a new small library written in Brasa, `bufio`, with no system calls, shared by files and sockets and also usable on its own (for example to split text that arrives in pieces).
- Unflushed data of a writer that is dropped without `flush()` or `close()` is discarded (a destructor cannot wait for the system); the docs say to close writers.

## Capabilities

### New Capabilities

- `buffered-io`: buffered readers and writers over files and TCP connections, line reading, and the `bufio` buffers they are built on.

### Modified Capabilities

None.

## Impact

- New `std/bufio.br` (registered in `std_source` and the interpreter loader; pure Brasa, so it also runs in the interpreter), additions to `std/fs.br` and `std/net.br`.
- New `tests/buffered_io.rs`, `examples/lines.br`, a section in `docs/stdlib.html` (and `docs/async.html` where files and sockets are described), README rows.
