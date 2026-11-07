## Purpose

Lets programs read files and connections line by line and write many small pieces cheaply, with buffers that talk to the system in large pieces.

## Requirements

### Requirement: Reading a file by lines
`fs.open(path)` SHALL return an `fs.Reader` for an existing file, or an error naming the path and the reason when it cannot be opened. `read_line()` SHALL return the next line without its terminator (`\n` or `\r\n`), SHALL return the last line even when it has no terminator, SHALL return empty text for an empty line, and SHALL return an error whose message is `end of file` when no data is left. Text is bytes, so a multibyte character is never split inside a line. Lines longer than the buffer SHALL be returned whole. `at_end()` SHALL tell whether anything is left to read.

#### Scenario: Lines of different endings
- **WHEN** a file holds `one\ntwo\r\n\nlast` and a program reads lines until the error
- **THEN** it gets `one`, `two`, an empty line and `last`, and then the `end of file` error

#### Scenario: Missing file
- **WHEN** a program opens a path that does not exist
- **THEN** it gets an error that names the path and the reason Windows gives

#### Scenario: Long and multibyte lines
- **WHEN** a file has a line of 200,000 characters and a line with `ação` repeated
- **THEN** both come back whole and unchanged

#### Scenario: Empty file
- **WHEN** an empty file is opened
- **THEN** `at_end()` is true and `read_line()` is the `end of file` error

### Requirement: Reading bytes
`read(max)` SHALL return up to `max` bytes (at least one unless the data has ended, in which case empty text) taking first what is already buffered, and `read_all()` SHALL return everything that is left. Mixing `read_line`, `read` and `read_all` on one reader SHALL continue where the previous call stopped.

#### Scenario: Mixing the calls
- **WHEN** a program reads one line, then 3 bytes, then the rest of a file
- **THEN** the three results are consecutive parts of the file with nothing lost or repeated

### Requirement: Few system reads
A reader SHALL ask the system for data in pieces of 64 KiB, so reading a file line by line performs about one system read per 64 KiB of the file (the reader counts them in `reads`).

#### Scenario: Many short lines
- **WHEN** a file of 100,000 short lines (about 1 MB) is read line by line
- **THEN** all lines arrive, in order, and `reads` is at most the file size divided by 65,536, rounded up, plus one

### Requirement: Buffered writing to a file
`fs.create(path)` SHALL create or replace a file and `fs.append(path)` SHALL open it for writing at its end, each returning an `fs.Writer` or an error naming the path. `write(text)` and `write_line(text)` (which adds `\n`) SHALL collect data in a buffer and SHALL give it to the system only when 64 KiB have gathered, on `flush()` or on `close()` (the writer counts system writes in `writes`). `close()` SHALL flush first. A writer that is dropped without `flush()` or `close()` MAY lose its buffered data.

#### Scenario: Many small writes
- **WHEN** a program writes 100,000 short lines and closes the writer
- **THEN** the file holds all the lines in order and `writes` is at most the data size divided by 65,536, rounded up, plus one

#### Scenario: Nothing before the flush
- **WHEN** a program writes a short text and has not flushed
- **THEN** `writes` is still 0 (the system has been given nothing); after `flush()` it is 1 and the file holds the text, and a second `flush()` with nothing buffered does not write again

#### Scenario: Append
- **WHEN** a file holds `a\n` and a program appends `b` with `fs.append`
- **THEN** the file holds `a\nb`

### Requirement: Readers and writers over connections
`conn.reader()` SHALL return a `net.Reader` and `conn.writer()` a `net.Writer` with the same methods and rules as the file ones (for a reader, `at_end()` and `read_line()` wait for the peer, and the end of data is the peer closing the connection). A writer's `flush()` SHALL send everything that is buffered; a connection writer's `close()` SHALL flush and SHALL leave the connection open (it is closed with `conn.close()`).

#### Scenario: A line protocol
- **WHEN** a client writes `hello` and `world` with `write_line`, flushes, and a server reads two lines from the accepted connection
- **THEN** the server gets `hello` and `world`, even if they arrived in one packet or in several

#### Scenario: Lines split across packets
- **WHEN** the peer sends `ab`, then `c\nd`, then `e\n` in three writes
- **THEN** `read_line()` gives `abc` and then `de`

### Requirement: The bufio buffers
`use bufio` SHALL provide, without any system call, an input buffer that accepts text in pieces (`feed`), marks the end of data (`finish`) and hands out lines (`has_line`, `take_line`), bytes (`take(max)`) and the rest (`take_all`), and an output buffer (`add`, `len`, `take`) that accumulates text. They SHALL behave the same in the interpreter and in native programs.

#### Scenario: Pieces to lines
- **WHEN** an input buffer is fed `ab`, `c\nd` and `e\n`
- **THEN** it has the line `abc`, then the line `de`, and then no line

#### Scenario: The last line
- **WHEN** an input buffer holds `tail` without a terminator
- **THEN** `has_line()` is false until `finish()` is called, after which `take_line()` gives `tail`

### Requirement: No leaks
Readers, writers and buffers SHALL free their memory, and a reader or writer SHALL close the file handle or connection reference it holds when it is dropped.

#### Scenario: Repeated use
- **WHEN** a program opens, reads and closes a file thousands of times
- **THEN** `live_objects()` does not grow
