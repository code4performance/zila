## 1. The bufio core

- [x] 1.1 Create `std/bufio.br`: `InBuf` (`feed`, `finish`, `has_line`, `take_line`, `take`, `take_all`) and `OutBuf` (`add`, `len`, `take`)
- [x] 1.2 Register `bufio` in `std_source` and in the interpreter's loader
- [x] 1.3 Tests of the buffers, natively and in the interpreter (pieces to lines, last line, `\r\n`, empty lines, multibyte, `take`, `take_all`)

## 2. Files

- [x] 2.1 `fs.open` and `fs.Reader` (`read_line`, `read`, `read_all`, `at_end`, `close`, `reads`)
- [x] 2.2 `fs.create`, `fs.append` and `fs.Writer` (`write`, `write_line`, `flush`, `close`, `writes`), including the append open mode
- [x] 2.3 File tests: every ending, empty file, missing file, long and multibyte lines, mixed calls, counters on 100,000 lines, append, nothing before flush

## 3. Sockets

- [x] 3.1 `conn.reader()` with `net.Reader` and `conn.writer()` with `net.Writer`
- [x] 3.2 Socket tests: a line protocol, lines split across packets, flush, and the peer closing

## 4. Leaks, example and documentation

- [x] 4.1 Leak test (open, read, close in a loop)
- [x] 4.2 `examples/lines.br` and sections in `docs/stdlib.html` and `docs/async.html`
- [x] 4.3 README rows, full test suite and `openspec validate add-buffered-io`
