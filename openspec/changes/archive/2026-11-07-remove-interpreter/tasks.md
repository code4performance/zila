## 1. Move what stays

- [x] 1.1 Create `src/float_text.rs` with `float_text`, `float_text_f32` and `layout_float` moved unchanged from `src/interp.rs`; in `src/lib.rs` replace `pub use interp::{float_text, float_text_f32, Interpreter}` with `pub use float_text::{float_text, float_text_f32}`. Verify: `cargo test --test floats --test stdlib` (the tests that call `zila::float_text` pass unchanged)

## 2. The new `--run`

- [x] 2.1 Add `src/run.rs` (`pub mod run;` in `src/lib.rs`) with `run_file(file, optimize, target) -> i32`: build into `temp_dir()/zila-run-<pid>-<n>[.exe]` with `compile_to_native_with` or `compile_to_linux`, chmod 755 on Linux, run with inherited stdio, delete through a `Drop` guard, return the exit code (`128 + signal` on Linux). Verify: `tests/run.rs` (task 2.3)
- [x] 2.2 `src/cli.rs` and `src/main.rs`: record whether `--target` was explicit; replace the `Mode::Run` branch (the 512 MiB interpreter thread) with a call to `run::run_file`; compile errors print `compilation error: ...` and exit 1; a `--target` other than the host is rejected before compiling. Add a unit test in `src/cli.rs` for the explicit-target flag. Verify: `cargo test --lib cli`; `zila-parse examples/hello.br --run` prints the greeting; `--run --target linux` on Windows fails with the message
- [x] 2.3 New `tests/run.rs`: pass-through of stdout and stderr, exit status 0 and 101 (panic), compile error runs nothing and exits 1, mismatched `--target` rejected, no temporary file left (own `TMP`/`TMPDIR`), standard input reaches the program. Verify: `cargo test --test run`

## 3. Delete the interpreter

- [x] 3.1 Delete `src/interp.rs`; remove `pub mod interp;` from `src/lib.rs`. Verify: `cargo build` and `cargo clippy` clean, and `grep -rn "Interpreter\|interp::" src tests` finds nothing
- [x] 3.2 Delete `tests/parse.rs::interpreter_runs_hello` (replaced by 2.3). Verify: `cargo test --test parse`

## 4. Rewrite the tests that ran the interpreter

- [x] 4.1 `tests/buffered_io.rs`, `tests/format.rs`, `tests/sets.rs` (the `interpreted` helper), `tests/sorted.rs`: replace the `--run` call with the file's native `run` helper and the same expected text; delete helpers that become unused. Verify: `cargo test --test buffered_io --test format --test sets --test sorted`
- [x] 4.2 `tests/defer.rs` (the two tests that pass `--run`): run natively; expected output unchanged. Verify: `cargo test --test defer`
- [x] 4.3 `tests/floats.rs::the_interpreter_agrees_with_the_native_compiler`, `tests/generics.rs` (the `examples/generics.br` test), `tests/maps.rs` (interpreter half of the map test, and its module comment), `tests/stdlib.rs` (`examples/words.br`): keep the native half, assert the expected text directly, rename tests so none mentions the interpreter. Verify: `cargo test --test floats --test generics --test maps --test stdlib`
- [x] 4.4 Confirm no test depends on the interpreter. Verify: `grep -rn -i "interpreter\|\-\-run" tests` shows only `tests/run.rs`, then a full `cargo test` without `tests/async.rs`

## 5. Docs

- [x] 5.1 `README.md`: remove the "Interpreter (`--run`)" status row, change the usage line to "compile and run it" and remove `interp.rs` from the source layout. Verify: `grep -n -i "interpret" README.md` finds only text-interpolation uses
- [x] 5.2 `docs/getting-started.html`: reword the `--run` comment and its `<dt>`/`<dd>` entry (builds a temporary executable for this machine, runs it, passes the exit status), remove "tree-walking interpreter" from the introduction. Verify: `cargo test --test docs` (checks every flag in `src/cli.rs` is described)
- [x] 5.3 `docs/native.html` (paragraph saying the interpreter is separate), `docs/stdlib.html` (the two interpreter notes, including `format.fmt` in the interpreter), `docs/syntax.html` (the "Not everywhere" float note and the maps note): delete the interpreter caveats. Verify: `grep -n -i "interpreter" docs/*.html` has no hits and `cargo test --test docs` passes
- [x] 5.4 `grep -n "\-\-run" examples/*.br` (only `examples/sorted.br`): keep the comment, it stays true; if any example changes run `zila-parse --docs`. Verify: `cargo test --test docs`

## 6. Final check

- [x] 6.1 `cargo build --release`; `zila-parse examples/hello.br --run` and `examples/generics.br --run` work; time `--run` on `hello.br` and record it in the pull request; `std/`, `os/` and `zila-ffi.md` are unchanged by design. Verify: `openspec validate remove-interpreter --strict`
