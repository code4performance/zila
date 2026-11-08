## Context

`--run` parses the file, builds an `Interpreter`, loads the file and calls `main` on a thread with a 512 MiB stack (`src/main.rs`, `Mode::Run`). The interpreter has its own value model, its own copies of library behavior and its own float printing (`float_text`, `layout_float`), which tests use as the reference for the native `float.to_text`. `--build` goes through `compile_to_native_with` (Windows) or `compile_to_linux`; `--test` (`src/testrun.rs`) already builds and runs executables.

Which tests really execute the interpreter (found by grepping `--run` and `Interpreter` in `src/`, `tests/`, `docs/`, `README.md`, `CONTRIBUTING.md`, `zila-ffi.md`): `tests/parse.rs::interpreter_runs_hello` (library call), and the CLI tests that pass `--run`: `buffered_io.rs`, `defer.rs` (two), `floats.rs` (`the_interpreter_agrees_with_the_native_compiler`), `format.rs`, `generics.rs`, `maps.rs`, `sets.rs`, `sorted.rs`, `stdlib.rs`. The `run()` helpers in `async.rs`, `dns.rs`, `udp.rs`, `tls.rs`, `ui.rs` and others are native builds and are unaffected. `CONTRIBUTING.md` and `zila-ffi.md` do not mention `--run` or the interpreter.

**Depends on:** `add-direct-pe-output` (a fast, dependency-free host build on Windows; without it `--run` would need Visual Studio). **Prerequisite for:** `add-comptime-reflection`. No overlap with `remove-async-coloring`, `add-error-kind` or the other planned changes: they change what the compiler accepts, and after this change they never have to be mirrored in a second implementation.

## Goals / Non-Goals

**Goals**
- `--run` runs the real compiled program: same output, same panics, same exit status as running the executable `--build` makes.
- Delete the interpreter and everything that only it needed.

**Non-Goals**
- Any compile-time execution (`comptime`), a REPL, or a cache of built executables.
- Running a program built for another target (no WSL, no emulator).
- Passing command-line arguments to the program (the language has no way to read them yet).

## Decisions

1. **Temporary executable.** `--run` builds into a fresh file in `std::env::temp_dir()` named `zila-run-<pid>-<counter>` (with `.exe` on Windows), using the same optimizer setting and compile functions as `--build`. It is deleted after the program ends, also when the program or the build fails. A `Drop` guard does the deletion so a panic in the driver still cleans up. On Linux the file is marked executable (`0o755`).
2. **Host target only.** The host is chosen with `cfg!(windows)` / `cfg!(target_os = "linux")`. `--target` defaults to `windows` today; for `--run` the default becomes "the host", and an explicit `--target` that differs from the host fails with a message such as `--run can only run programs built for this machine (windows); use --build --target linux`. Other hosts get "--run is not supported on this platform" until a target exists. The check happens before compiling. (`src/cli.rs` keeps `target` as a `String`; to tell "explicit" from "default" the parser records whether `--target` was given.)
3. **Pass-through.** The child inherits stdin, stdout and stderr (`Command::status`), so output is not buffered by the tool and interactive programs work. The tool exits with the child's exit code; on Linux a child ended by a signal gives `128 + signal`, as a shell does. The old `runtime error:` prefix goes away because the program prints its own `panic: ...` line.
4. **Where the code lives.** `run::run_file(file, optimize, target) -> i32` in a new `src/run.rs` (about 40 lines; `main.rs` is already large). It reuses the existing compile entry points; it does not shell out to `--build`.
5. **Float reference layout.** `float_text`, `float_text_f32` and `layout_float` move verbatim to `src/float_text.rs`. They are not part of the language implementation: they are the Rust-side oracle the tests compare native output against, so they stay public and `src/lib.rs` re-exports them with the same names; the call sites in `tests/floats.rs` and `tests/stdlib.rs` do not change.
6. **Behavior differences users will see** (all toward the shipped program): programs the interpreter accepted but the compiler rejects now fail with the compiler's message; stack depth is the executable's, not the interpreter's 512 MiB thread; execution is native but start-up includes a build.
7. **Test rewrite rules.**
   - A test that only ran `--run` and checked the output moves to the native `run` helper in that file (same expected text).
   - A test that compared `--run` with native (`floats`, `generics`, `maps`, `stdlib`) keeps the native half and asserts the expected text directly.
   - `tests/parse.rs::interpreter_runs_hello` is deleted; `tests/run.rs` replaces it by running `examples/hello.br` through the binary.
   - `tests/run.rs` is new: output pass-through, exit status (a panic gives 101 and its `panic: ...` line on standard output, where compiled programs print it; the language has no standard-error writer, so the stderr and stdin tests use `extern` calls to the system), a compile error runs nothing and exits 1, a mismatched `--target` is rejected, the temporary file is gone afterwards (the test points `TMP`/`TMPDIR` at its own directory and checks it is empty), standard input reaches the program.
8. **Spec cleanup.** Each "the interpreter agrees" requirement is REMOVED with a note on what replaced it. `format-library`'s "A table" and "Typed formatting is native" are removed outright: the second one only existed to let the interpreter lack `format.fmt`.

## Risks / Trade-offs

- `--run` is slower than before because it builds. Mitigation: it relies on the dependency-free builder and spawns nothing but the program; time `examples/hello.br` and record it.
- Until `add-direct-pe-output` lands, `--run` on Windows would call the Visual Studio toolchain; this change must not be applied before it.
- Antivirus or a read-only temp directory can block running a freshly written executable; the error names the path and the operating system's message, and `--build` remains the fallback.
- Concurrent `--run` calls are safe because the name includes the process id and a counter.
