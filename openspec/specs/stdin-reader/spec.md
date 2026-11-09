# stdin-reader Specification

## Purpose
Let a Zila program read what is piped or typed into it, with the same reader API it already uses for files.

## Requirements

### Requirement: fs.stdin gives a reader over standard input
`fs.stdin()` SHALL return an `fs.Reader` that reads the process's standard input, with the methods `read_line()`, `read(max)`, `read_all()`, `at_end()` and `close()` behaving as they do for a reader of a file. Standard input MAY be a pipe, a redirected file or a terminal, on Windows and on Linux.

#### Scenario: Lines from a pipe
- **WHEN** a program loops on `read_line()` of `fs.stdin()` and `a\nb\r\nc` is piped in
- **THEN** it sees `a`, `b` and `c` in order (without line endings), then the end of the input

#### Scenario: Everything at once
- **WHEN** a program calls `read_all()` on `fs.stdin()` with `hello` piped in
- **THEN** it gets `hello`

#### Scenario: Redirected file
- **WHEN** standard input is a file of 200 000 bytes and the program calls `read_all()`
- **THEN** it gets all 200 000 bytes

### Requirement: The end of the input is an ordinary end
At the end of standard input `at_end()` SHALL return true, `read(max)` SHALL return empty text and `read_line()` SHALL give an error of kind `UnexpectedEof` with the message `end of file`, whether the input was a closed pipe, an empty pipe or an empty file. A last line without a newline SHALL still be returned.

#### Scenario: Empty input
- **WHEN** nothing is piped in and the program calls `at_end()`
- **THEN** it returns true and `read_line()` gives `end of file`

#### Scenario: No final newline
- **WHEN** `x` without a newline is piped in
- **THEN** `read_line()` returns `x` and the next call gives `end of file`

### Requirement: Reading standard input waits for the whole program
Reading standard input SHALL wait until data or the end of input arrives, without returning early and without an error. While it waits, other tasks of the program do not run; this SHALL be stated in the documentation.

#### Scenario: Slow producer
- **WHEN** a producer writes a line, waits a moment, then writes another, and the program reads two lines
- **THEN** it returns both lines, in order

### Requirement: Closing never closes standard input
`close()` on the reader of standard input SHALL end that reader and SHALL NOT close the process's standard input; dropping the reader SHALL not close it either.

#### Scenario: Two readers
- **WHEN** a reader from `fs.stdin()` is closed and `fs.stdin()` is called again
- **THEN** the new reader works on the same standard input, without an error (it finds whatever the first reader left unread, here the end of the input)

### Requirement: Both platform layers expose the same surface
`std/fs.zila` and `std/fs.linux.zila` SHALL both define `stdin` and the new public field of `Reader`, so the parity test between the layers keeps passing without new entries in `tests/data/std_parity_known.txt`.

#### Scenario: Parity
- **WHEN** the parity test runs
- **THEN** it reports no new difference between the layers

### Requirement: The commands that run programs feed standard input through
A program started by `zila run` or `zila -e` SHALL read the terminal's or the pipe's standard input through `fs.stdin()`, except that `zila run -` and `zila eval -` have already consumed it for the program's source, so the program sees an empty input.

#### Scenario: Filter through eval
- **WHEN** `printf 'a\nb\n' | zila -e 'let r = fs.stdin()' ...` reads and prints each line in upper case
- **THEN** it prints `A` and `B`

#### Scenario: Source from standard input
- **WHEN** a program started with `zila run -` calls `at_end()` on `fs.stdin()`
- **THEN** it returns true
