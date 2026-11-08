## Why

The README, docs and every example write `int`, and `zila.ebnf` says "integer literals default to i32", yet the compiler today treats `int` as `i64` (`int_type` in `src/codegen_x64.rs`), types integer literals as `i64` (`Ty::i64()`), and `docs/syntax.html` calls `int` "64-bit". Spec, grammar notes, docs and compiler disagree. The owner decided: `int` is a named alias of `i32` and `long` is a named alias of `i64`, so the everyday names have one documented size that matches C and the sized types.

## What Changes

- **BREAKING**: `int` becomes `i32` (it was `i64`). Integer literals default to `int` (`i32`) when nothing else fixes the type (they were `i64`).
- New name `long` = `i64`. Both are aliases, not distinct types: `int` and `i32` (and `long` and `i64`) are interchangeable everywhere, including interface satisfaction, generics, map keys and `extern` signatures.
- `float` stays an alias of `f64` (already true); the spec now states it.
- Error messages show `int` and `long` for `i32` and `i64`; other sized types keep `i8`, `u16`, and so on.
- `int(x)` and `long(x)` are conversions exactly like `i16(x)`; they do not clash with the type names.
- Lossless widening is implicit (`int` to `long`, `u32` to `i64`, mixed arithmetic takes the wider type); narrowing, same-width signed/unsigned and integer/float stay explicit. Arithmetic is computed in the operand type. `zila.ebnf`'s "no implicit numeric coercion" becomes "no implicit lossy coercion". This keeps the migration small: most call sites that pass counts and offsets keep compiling.
- `len`, counts and list indices are `int`; a collection holds at most 2,147,483,647 elements and the runtime stops the program past that. Sizes of files, offsets, timestamps, byte counters and hashes are `long`.
- The standard library, `os/`, `examples/`, tests and docs are rewritten where an `int` was a 64-bit accumulator or a value that can exceed `i32` (timestamps, sizes, byte counts, hashes) to use `long` or `usize`.
- `zila.ebnf` semantic notes and `docs/syntax.html` are corrected.

## Capabilities

### New Capabilities
- `integer-types`: the integer and float type names, their aliases, literal defaults, conversion functions and type display in errors.

### Modified Capabilities
<!-- None: `floating-point` already says `float` is identical to `f64` and its int wording stays true. -->

## Impact

- **Compiler** (`src/codegen_x64.rs`): `int_type`, `Ty::i64()` uses for literals and `len`/index results, `Ty::show`, the hint in the generic inference error (line ~5485). `c_type` (the `c.int`/`c.long` C-ABI table) is unchanged.
- **Library and code**: `std/*.br`, `os/*.br`, `examples/*.br`, `tests/*.rs`, `README.md`, `docs/*.html`, `zila.ebnf`.
- **Assumptions**: `usize`, `isize` and `u64` already exist (`int_type`), so no new type is invented. `--run` is not updated (the interpreter is removed by `remove-interpreter`).
