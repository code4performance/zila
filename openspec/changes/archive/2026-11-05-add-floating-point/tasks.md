## 1. Assembler

- [x] 1.1 Add SSE2 data movement (`movq` xmm/r64, `movsd`, `movss`, `movd`) to `asm_x86.rs`; verify with byte-level unit tests against known encodings
- [x] 1.2 Add arithmetic and logic (`addsd/subsd/mulsd/divsd`, `ss` forms, `sqrtsd`, `xorpd`, `andpd`); verify with byte-level unit tests
- [x] 1.3 Add comparison, conversion and rounding (`ucomisd/ucomiss`, `cvtsi2sd`, `cvttsd2si`, `cvtsd2ss`, `cvtss2sd`, `roundsd`); verify with byte-level unit tests

## 2. Types and literals

- [x] 2.1 Add `Ty::Float { bits }`, resolve `float`/`f32`/`f64`, make it non-rc and 8 bytes in slots, and reject float map keys; verify with compile tests for valid declarations and the key error
- [x] 2.2 Compile float literals (data-section constants, `0.0` shortcut, `f32` by expected type) and reject integer literals where a float is expected, with the hint; verify with a program returning a literal and a compile-error test

## 3. Operations

- [x] 3.1 Compile `+ - * /` and unary minus for `f64` and `f32`, rejecting `%` and mixed types; verify with a program that checks results through comparisons and compile-error tests
- [x] 3.2 Compile the six comparisons with correct NaN behavior; verify the ordering cases and the NaN cases from the spec
- [x] 3.3 Compile `float(x)`, `int(x)` (with the `float out of range` panic), `f32(x)` and `f64(x)`; verify truncation, negative values, the infinity panic and narrowing

## 4. Printing

- [x] 4.1 Write `rt_float_to_text` (shortest round-trip digits, `.0` suffix, exponent thresholds, `NaN`/`inf`/`-0.0`) and hook it into `to_text` for `print`, `println` and interpolation; verify with the spec's printing scenarios
- [x] 4.2 Verify `f32` printing and a native-versus-interpreter comparison over a few thousand varied bit patterns

## 5. Calls and data structures

- [x] 5.1 Verify floats as parameters, returns, record fields, list elements, map values, closure captures and spawned-task arguments, with no leaks under `live_objects()`
- [x] 5.2 Pass and return floats in `extern` calls using XMM registers (positional, beyond four on the stack, variadic mirroring); verify with an extern call to the C runtime's `sqrt` and a four-float-argument call

## 6. Standard library

- [x] 6.1 Add compiler intrinsics for `sqrt`, `floor`, `ceil` and `trunc`, and `std/math.br` with `abs`, `min`, `max`, `round`, `pow` and `fixed`; verify with the helper scenarios including `sqrt(-1.0)`

## 7. Interpreter and other targets

- [x] 7.1 Add float values, literals, operations, conversions, printing and math helpers to `interp.rs`; verify `--run` output equals native output for `examples/floats.br`
- [x] 7.2 Reject floats on the bare-metal target with the documented message; verify with a compile-error test

## 8. Documentation and examples

- [x] 8.1 Add `examples/floats.br` and its section on `docs/examples.html` (via `zila-parse --docs`); verify it builds and prints the documented output
- [x] 8.2 Document floats on `docs/syntax.html` and `docs/native.html` (removing them from the not-yet-supported lists), and update the status table and roadmap in `README.md`; verify `cargo test` passes, including the docs tests
