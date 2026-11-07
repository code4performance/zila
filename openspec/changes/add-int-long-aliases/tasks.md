## 1. Compiler

- [ ] 1.1 In `src/codegen_x64.rs` change `int_type` so `int` is 32-bit signed and add `long` as 64-bit signed; verify with compile tests that `let a: int = 1; let b: i32 = a` and `let c: long = 1; let d: i64 = c` compile, and that assigning an `int` to a `long` without conversion fails as a mismatch
- [ ] 1.2 Type unconstrained integer literals as `int` (`ExprKind::Int` at ~line 1338, replacing the `Ty::i64()` helper use) and keep range checks; verify `let x = 3000000000` is a range error suggesting `long`, and `let x: long = 3000000000` compiles
- [ ] 1.3 Make `Ty::show` print `int` and `long`, and update the inference hint at ~line 5485; verify with error tests showing `expected int, found text` and `expected long, found int`
- [ ] 1.5 Implement lossless widening in the type checker for assignment, initialization, arguments, returns, field initializers and binary operands (result takes the wider type), with none for narrowing, same-width signed/unsigned, integer/float, generic inference or interface and function-value signatures; verify with accept tests (`int` to a `long` parameter, `int + long` is `long`, `u32` to `i64`) and reject tests (`long` to `int`, `u64` to `int`, `i32` to `u64`, `float` to `int`, `max<T>(int, long)`) in `tests/codegen.rs`
- [ ] 1.6 Make `len`, element counts and list indices `int`, and stop the program with a fatal failure when a list, map or text would exceed 2,147,483,647 elements; verify that `for i in 0..xs.len` indexes with no conversion and that a test growing past the ceiling (using a synthetic limit in the runtime for the test) stops the program
- [ ] 1.7 Verify that `int * int` wraps at 32 bits before widening and that `long(a) * long(b)` is exact, with a test
- [ ] 1.4 Verify `int(x)`, `long(x)` and `float(x)` as conversions (float to `int` out of range still panics `float out of range` at the i32 limits) and that `c.int` / `c.long` keep C sizes; add tests in `tests/codegen.rs` and `tests/floats.rs`

## 2. Rewrite Brasa code

- [ ] 2.1 Review `std/*.br` and change every place a value can exceed `i32` (file sizes and offsets in `fs`, timestamps, hashes in `set`/maps, byte counters, handles) to `long` or `usize`, leaving lengths and indices as `int`, and list every `int` accumulator (hash functions, `format` thousands, time and size arithmetic) in the commit message; verify with the full test suite excluding `tests/async.rs` plus one test per library that exceeds 2^31
- [ ] 2.2 Do the same for `os/boot.br`, `os/kernel.br` and `examples/*.br`; verify with `tests/os.rs` and by compiling every example
- [ ] 2.3 Update expected output and literals in `tests/*.rs` that assumed 64-bit `int`; verify the suite passes (async tests only when the owner asks)

## 3. Docs and grammar

- [ ] 3.1 Fix `brasa.ebnf` semantic notes (literals default to `int` = `i32`; `long` = `i64`; `float` = `f64`) and `docs/syntax.html` (remove "int (64-bit)", document the aliases, `long`, `usize` and the 2 GB note); verify by grep that no doc says `int` is 64-bit
- [ ] 3.2 Update `README.md`, `docs/stdlib.html`, `docs/native.html` and `docs/examples.html` where they state sizes or use `int` for large values; verify the pages and the docs-sync checks (`docs-sync` spec) pass
