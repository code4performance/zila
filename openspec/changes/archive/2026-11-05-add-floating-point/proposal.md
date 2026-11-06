## Why

`float`, `f32` and `f64` are parsed and even lexed (`3.14`), but the native compiler answers "floating point is not supported yet". Programs cannot compute averages, percentages, geometry or timings, and the standard library cannot grow numeric helpers. It is item 1 on the roadmap, and the text/collection libraries and `ui` code want it next.

## What Changes

- **Types**: `float` (an alias for `f64`), `f64` and `f32` become native types for variables, parameters, returns, record fields, list elements, map values and `extern` signatures.
- **Literals**: `1.5`, `2e10`, `.5` style literals compile; their type is `float` unless the expected type is `f32`.
- **Arithmetic**: `+ - * /` and unary `-` on floats; `%` is rejected. Division by zero follows IEEE 754 (infinity or NaN), it does not panic.
- **Comparison**: `== != < <= > >=` on floats with IEEE semantics (any comparison with NaN is false except `!=`).
- **Conversion**: no implicit int/float mixing. Explicit `float(x)` and `int(x)` conversions (`int` truncates toward zero; NaN and out-of-range values panic with `float out of range`), plus `f32(x)`/`f64(x)` between float widths.
- **Printing and text**: `print`, `println` and string interpolation print floats in the shortest form that reads back exactly (`1.5`, `0.1`, `1e21`, `NaN`, `inf`, `-inf`); whole numbers print with `.0` (`3.0`).
- **Math helpers** in `std/math.br`: `sqrt`, `abs`, `floor`, `ceil`, `round`, `min`, `max`, `pow` (`sqrt` and rounding use SSE instructions; the rest are written in Brasa).
- **FFI**: `extern` functions take and return floats in the Windows x64 way (XMM registers).
- **Assembler**: SSE2 scalar instructions needed by the above are added to the built-in assembler.
- **Interpreter**: `--run` supports the same float behavior.
- Docs, an `examples/floats.br` program, and the README status table and roadmap are updated in the same change.

## Capabilities

### New Capabilities
- `floating-point`: the float types, literals, arithmetic, comparison, conversion, printing, math helpers, FFI passing and error cases.

### Modified Capabilities
<!-- None: integer, text and other behavior is unchanged. -->

## Impact

- **Assembler** (`src/asm_x86.rs`): SSE2 encodings (`movq`, `movsd`, `movss`, `addsd`/`subsd`/`mulsd`/`divsd` and `ss` forms, `sqrtsd`, `ucomisd`/`ucomiss`, `cvtsi2sd`, `cvttsd2si`, `cvtsd2ss`, `cvtss2sd`, `roundsd`, `xorpd`/`pxor`, `andpd`).
- **Compiler** (`src/codegen_x64.rs`): `Ty::Float { bits }`, type resolution, literals (constants in the data section), `binary`, unary minus, comparisons, conversions, `to_text`, extern calls.
- **Runtime**: `rt_float_to_text` (shortest round-trip formatting) and `rt_float_to_int` checks.
- **Interpreter** (`src/interp.rs`), **stdlib** (`std/math.br`), **docs/examples/tests**: `docs/syntax.html`, `docs/native.html`, `docs/examples.html`, `README.md`, `examples/floats.br`, new `tests/floats.rs`.
- **Assumptions** (no clarification needed):
  - `float` is exactly `f64`; `f32` exists mainly for FFI and storage.
  - No implicit conversion between int and float, matching how sized integers already behave.
  - Floats are not allowed as map keys (already the rule for maps).
  - Windows x64 and the hosted native target only. The bare-metal OS target rejects floats with a clear message (no SSE state setup there yet).
  - Not included: SIMD, `f16`/`f128`, hex float literals, a `format` function with precision (a `fixed(x, digits)` helper is included in `std/math.br`), trigonometry and `exp`/`log` (a later stdlib item).
