## Why

`src/interp.rs` (about 1,200 lines) is a tree-walking interpreter behind `--run`. It was the first backend and it has fallen behind the native compiler: it has no `f32`, no `math`, no `format.fmt`, no async, no tasks, no channels, no `extern`, no `match` on results and no lambdas that capture. Every "the interpreter agrees" requirement in the specs is a promise that has to be kept twice, and every new feature either skips the interpreter ("MAY be unable to use them") or pays for a second implementation. Worse, it is a second, wrong definition of the language: a program can pass `--run` and fail to compile, or the reverse.

There is one language and one compiler. After `add-direct-pe-output` the compiler builds a host executable with no external tools on both Windows and Linux, so `--run` can simply be "compile, run, delete" and give the exact behavior of the shipped program. It also removes the need to keep the interpreter in step for the planned `comptime` work (`add-comptime-reflection`), which runs compiled code at build time.

## What Changes

- **BREAKING**: `src/interp.rs` and the `Interpreter` type are deleted. `brasa::Interpreter` is no longer exported.
- `brasa-parse file.br --run` compiles the program to a temporary native executable for the **host** (a PE on Windows, an ELF on Linux), runs it with the terminal's standard input, output and error passed straight through, deletes the executable, and exits with the program's exit status (101 after a panic, as the program itself would). Optimization follows `--no-opt` as for `--build`.
- Compile errors are reported exactly as `--build` reports them (`compilation error: ...`, exit 1); nothing is run. The checks the compiler makes and the interpreter did not (types, generics, exhaustiveness) now apply to `--run` too.
- `--run` with a `--target` that is not the host is an error that says so (no emulation, no WSL).
- `float_text` and `float_text_f32` (the reference layout of a float used by tests) keep their public names and move from `interp.rs` to a new `src/float_text.rs`; `src/lib.rs` re-exports them from there.
- Tests that executed the interpreter are rewritten to run natively; tests that compared interpreter output with native output keep only the native half with the expected text written out. A new `tests/run.rs` covers `--run` itself.
- Documentation drops every statement about an "AST interpreter" and its limits (`README.md`, `docs/getting-started.html`, `docs/native.html`, `docs/stdlib.html`, `docs/syntax.html`).

## Capabilities

### New Capabilities

- `run-command`: what `--run` does: build for the host, run, pass through output and exit status, clean up, errors.

### Modified Capabilities

- `defer`, `floating-point`, `format-library`, `generics`, `list-library`, `maps`, `set-library`, `sorted-library`, `text-library`: the requirements that make the interpreter agree with the native compiler are removed (the one rule that remains, that `--run` is the native program, is stated once in `run-command`).
- `docs-site`: the getting-started requirement keeps its flags but describes `--run` as compile-and-run.

## Impact

- `src/interp.rs` deleted; `src/lib.rs` (`mod interp`, re-export); new `src/float_text.rs` and `src/run.rs`; `src/main.rs` (`Mode::Run` branch).
- Tests: `tests/parse.rs` (`interpreter_runs_hello`), `tests/buffered_io.rs`, `tests/defer.rs`, `tests/floats.rs`, `tests/format.rs`, `tests/generics.rs`, `tests/maps.rs`, `tests/sets.rs`, `tests/sorted.rs`, `tests/stdlib.rs`; new `tests/run.rs`.
- Docs: `README.md`, `docs/getting-started.html`, `docs/native.html`, `docs/stdlib.html`, `docs/syntax.html`.
- `std/`, `os/`, `examples/` and `brasa-ffi.md` need no change (they contain no interpreter-specific code; `examples/sorted.br` only mentions `--run` in a comment that stays correct).
- Cost: `--run` is now as slow as a build instead of instant.
