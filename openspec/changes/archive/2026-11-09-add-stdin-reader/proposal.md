## Why

A Zila program cannot read standard input: `fs` opens files by path and `net` reads sockets, and nothing reads handle 0 or file descriptor 0. That rules out filters (`cat file | zila run filter.zila`), line-oriented tools, and the new `zila eval` / `zila run` commands being useful in a pipeline. The `--run` spec already promises that standard input passes through to the program, but no program can use it.

## What Changes

- **`fs.stdin()`** returns an `fs.Reader` over standard input, with the same methods as a file reader: `read_line()`, `read(max)`, `read_all()`, `at_end()` and `close()`. Lines come without `\n` or `\r\n`; `read_line()` at the end gives the error `end of file` (kind `UnexpectedEof`), as for files.
- Works on Windows and Linux, from a pipe, a redirected file or a terminal.
- Reading standard input **blocks the whole program** (all tasks) until data or the end arrives; unlike files and sockets it does not go through the scheduler's completion port. This is stated in the documentation; a non-blocking version is future work.
- `fs.Reader` gets one new public field, `std: bool`, true for the reader of standard input (the Windows and Linux layers keep the same public surface).
- Closing the reader of standard input never closes the process's standard input.

## Capabilities

### New Capabilities
- `stdin-reader`: reading standard input from a program.

### Modified Capabilities

None. (`run-command` already requires that standard input passes through; this makes it usable.)

## Impact

`std/fs.zila`, `std/fs.linux.zila`, `tests/stdlib.rs` or a new `tests/stdin.rs`, `tests/eval.rs` (the pipe scenarios), `docs/async.html` (the buffered-reading section), `examples/`, `CHANGELOG.md`. No change to the compiler. No change to programs that do not call `fs.stdin()`.

Non-goals: reading from the terminal without blocking other tasks, line editing or prompts, raw/unbuffered console mode, writing helpers for standard output and error (`print` stays the way to write), and reading standard input in a bare-metal kernel.
