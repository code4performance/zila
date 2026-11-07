## 1. Abort in the compiler and runtimes

- [x] 1.1 Replace the `panic` built-in in `src/codegen_x64.rs` with `abort("message")` (type `nothing`; a user function named `abort` takes precedence); `panic(...)` becomes an ordinary unknown-function compile error. Verify: a program calling `abort("stop")` on line 3 prints `abort: <file>.br:3: stop` and exits 101; a program calling `panic("x")` fails to compile.
- [x] 1.2 Rename `rt_panic` to `rt_abort` in `RUNTIME_COMMON`, `RUNTIME_WIN`, `RUNTIME_BARE` and `RUNTIME_LINUX`; it takes a location text and a message, writes `abort: <location>: <message>` to standard error (a new `rt_write_err` next to `rt_write_out`) and calls `rt_exit(101)` with nothing else. Change the `rt_msg_panic` data to `abort: `. Verify: output goes to stderr and not stdout on both targets (a run with the streams split), and `defer` and `drop` output is absent.
- [x] 1.3 Give each checking site its own location: out-of-bounds index (callers of `rt_index_oob`), integer `/` and `%` zero checks, `rt_list_take` (`take`), map `m[key]` (`src/runtime_map.asm`; the caller passes the location), `int(float)` and `abort(...)` itself. The compiler loads the `file.br:line` text from the data section into a register before the failing jump or call. Verify: one test per site checks the exact message (`index out of bounds`, `division by zero`, `map key not found`, `take from an empty list`, `float out of range`) with the right line, under `--no-opt` and with an inlined helper (line of the original source).
- [x] 1.4 Out of memory (the `rt_msg_oom` paths in the allocators) calls `rt_abort` with no location, printing `abort: out of memory`. Verify with the out-of-memory test in `tests/linux.rs`, updated to the new text and still status 101.
- [x] 1.5 Make `MIN / -1` and `MIN % -1` wrap (results `MIN` and `0`) before `idiv` in the Windows and Linux runtimes and in generated division. Verify: `-9223372036854775807 - 1` divided by `-1` through variables prints `-9223372036854775808` and the remainder prints `0`, with and without `--no-opt`, on both targets.
- [x] 1.6 Check that a constant zero divisor is still not folded and that folded `+ - *` still wrap (no code change expected). Verify with the existing tests in `tests/optimizer.rs`, adjusted for the new message.

## 2. Assertions

- [x] 2.1 Add a `test_mode` option to the compiler options, set by `src/testrun.rs` when it builds the test entry point (not exposed in the command line). Verify by a test that the default is off and that `--test` turns it on.
- [x] 2.2 In `assert_stmt`, keep the current `return error("line N: ...")` rewrite when `test_mode` is on and the function returns `error` or `T or error`; otherwise compile to an abort with `assertion failed`, the given message, or `assertion failed: left X, right Y`, at the assertion's location. Remove the "needs a function that returns an error" compile error. Verify: `tests/testrun.rs` results are unchanged (`FAIL name: line 7: assertion failed: left 3, right 4`); a normal build of a program with `assert(false)` in a function returning nothing compiles and aborts with `assertion failed`; under `--test`, a failing assert in a non-error helper is reported as an early stop with exit code 101.
- [x] 2.3 Update the early-stop note in `src/testrun.rs` so it names an abort rather than a panic; the exit status stays 1. Verify with the crashing-test scenario (a test dividing by zero).

## 3. Wrapping and checked arithmetic

- [x] 3.1 In `brasa.ebnf`, replace "Integer overflow: panic in debug, wraparound with --fast." with "Integer arithmetic wraps at the width of the type." Verify: `cargo test --test docs` and `cargo test --test parse` pass and `rg "panic in debug" .` finds nothing.
- [x] 3.2 Add `checked_add`, `checked_sub`, `checked_mul`, `checked_div` to `std/math.br`, written in Brasa, returning `int or error` (kind `Overflow` for overflow and `InvalidInput` for a zero divisor once `add-error-kind` has landed; before that, plain `error("overflow")` and `error("division by zero")`). Verify in `tests/abort.rs` (with the other tests of this change; `tests/stdlib.rs` has none of the integer functions): results at `MAX + 1`, `MIN - 1`, `MIN * -1`, `MAX * 2`, `MIN / -1`, division by zero, and an ordinary case for each function, with and without `--no-opt`.
- [x] 3.3 Verify wrapping is identical in all builds: a test prints `9223372036854775807 + 1` computed at run time (through a function parameter) and by folding, for `--no-opt`, the default, and `--target linux`, plus an `i32` wrap at its width.

## 4. Lists

- [x] 4.1 Add the `get(i)` list method returning `T?` (for element types that can be nil; `T or error` for the others, see design 7) next to `take` in `src/codegen_x64.rs`: unsigned compare against the length, `nil` when out of range or negative, otherwise the element with the same ownership as `xs[i]`. Verify: `[1, 2, 3].get(1)` is `2`, `get(3)` and `get(-1)` are `nil`; lists of texts and of records leave `live_objects()` unchanged after many calls; the result narrows after a nil check.
- [x] 4.2 Verify slices from `list.slice` accept `get`: slice, then call `get` in and out of range.
- [x] 4.3 Confirm `Map.get(key, default)` is unchanged: the existing map tests pass untouched.

## 5. Rewrite the repository

- [x] 5.1 `std/text.br`: change `panic("split needs a separator")` to `abort("split needs a separator")`. Verify: `rg "panic\(" std os examples` finds nothing, and the text-library test for an empty separator expects `abort: text.br:N: split needs a separator` and exit 101.
- [x] 5.2 Update expectations in `tests/floats.rs` (`out_of_range_conversion_panics`, rename to `..._aborts`), `tests/maps.rs` (`missing_key_panics` and the second `panic: map key not found` check), `tests/optimizer.rs` (the division-by-zero test), `tests/linux.rs` (the tests using `panic("stop")` become `abort("stop")`, the dynamic-program test, division by zero, out of memory). Verify: `cargo test` passes, leaving out `tests/async.rs`.
- [x] 5.3 (not applicable: `remove-interpreter` landed first and `src/interp.rs` is gone) In `src/interp.rs`, rename the `"panic"` entries (the native-name list near line 261 and the `"panic" =>` case) to `abort` only if the file still exists (`remove-interpreter` deletes it). Verify: `--run` prints `abort: ...` and exits 101 on a failing program, or the task is marked not applicable because the file is gone.
- [x] 5.4 Sweep for leftovers. Verify: `rg -i "panic" src std os examples docs README.md brasa.ebnf openspec/specs` shows no Brasa-language mention (Rust `panic!` in test helpers is allowed), and `cargo test --test os` still passes with the kernel's own division-fault handling unchanged.

## 6. Documentation and examples

- [x] 6.1 `docs/syntax.html`: rewrite the error-handling paragraph (replace `panic("message")` with the three-rule story: wrap, values, abort; list what aborts and that nothing runs), the `int(x)` conversion note, the missing-key note and the `defer` "where it does not run" note. Document `abort`, `get` and `checked_*`. Verify: `cargo test --test docs` passes and every code sample compiles.
- [x] 6.2 `docs/native.html` (exit-status line), `docs/stdlib.html` (the `split` note, `checked_*`, list `get`) and the `README.md` status: replace `panic` wording. Verify with the docs tests as above.
- [x] 6.3 Extend `examples/errors.br` with `get`, `checked_add` and a guarded `abort`, and regenerate `docs/examples.html` with `brasa-parse --docs`. Verify the example builds on both targets and prints the documented output.

## 7. Final check

- [x] 7.1 Run the full suite without `tests/async.rs`, then `openspec validate remove-panic-add-abort --strict`. Verify both pass.
