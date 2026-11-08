## Context

Every value in the native backend travels through `rax` and a stack slot as 64 bits (`self.expr` returns its `Ty`, the value is in `rax`). `Ty::Int { bits, signed }` normalizes after arithmetic. The assembler (`asm_x86.rs`) is hand written and has no SSE. The parser and lexer already produce `ExprKind::Float(f64)` and `Type::Named` for `float`/`f32`/`f64`; the backend rejects both in two places.

## Goals / Non-Goals

**Goals**
- Floats that feel like any other value: same slots, same calling convention inside Zila Lang, no reference counting.
- Correct IEEE behavior, exact round-trip printing, FFI that works.

**Non-Goals**
- Optimizing float code (the roadmap's optimizer item), SIMD, trigonometry, `f16`.

## Decisions

1. **`Ty::Float { bits: 32 | 64 }`.** `float` resolves to `Float { bits: 64 }`. Not rc, size 8 in slots.
2. **Bit patterns in `rax`.** A float lives in `rax` as its raw bits (`f32` in the low 32 bits, upper zero). Operations do `movq xmm0, rax` / `movq xmm1, rcx`, operate, and `movq rax, xmm0`. Slots, lists, records, closures, channels and task arguments stay unchanged, at the cost of a few extra moves (acceptable until the optimizer lands).
3. **Binary ops** reuse the existing "evaluate lhs, save, evaluate rhs, restore" shape in `binary`; for floats it selects `addsd/subsd/mulsd/divsd` (or `ss`). Comparisons use `ucomisd`/`ucomiss` then `setcc`, handling the parity flag so NaN gives false: `==` is `sete` AND `setnp`; `!=` is `setne` OR `setp`; `<`/`<=` swap operands and use `seta`/`setae`; `>`/`>=` use `seta`/`setae` directly (already false when unordered).
4. **Literals** are loaded as immediates holding the IEEE bits (`mov rax, <bits>`, `xor eax, eax` for `0.0`); no data-section constants are needed. A literal is `float` unless `want` is `f32`.
5. **No implicit conversion.** `check` already compares types exactly, so `int` vs `float` fails with the normal mismatch message; the literal case adds the hint to use `3.0` or `float(3)`.
6. **Conversions** are call-syntax built-ins next to `print`/`error`: `float(int)` is `cvtsi2sd`, `int(float)` is `cvttsd2si` plus a range check (it returns `0x8000000000000000` on failure, which triggers a closer boundary test and `rt_panic("float out of range")`), `f32`/`f64` are `cvtsd2ss`/`cvtss2sd`. Sized integer targets then use the existing `norm`.
7. **Printing** is `float.to_text(bits, single)` in `std/float.br`, written in Zila Lang and linked only when a program mentions floats. It is the exact free-format algorithm (Steele & White / Burger & Dybvig) over big numbers kept as lists of base-1e9 limbs, so it needs no tables and no assembly. Formatting rules (`.0` suffix, exponent form at `>= 1e16` or `< 1e-4`, `NaN`/`inf`/`-0.0`) are applied to the digit string. `to_text` dispatches on `Ty::Float`. A faster algorithm (Ryu) can replace the search later without changing behavior.
8. **Math helpers.** `sqrt` is `sqrtsd`, and `floor`/`ceil`/`trunc` use `roundsd` (SSE4.1, present on every CPU Windows 11 runs on). They are compiler intrinsics (`__sqrt`, `__floor`, `__ceil`, `__trunc`) wrapped by `std/math.br`; `abs`, `min`, `max`, `pow`, `round` and `fixed` are written in Zila Lang. `fixed` scales, rounds and formats integer digits, so no general formatter is needed.
9. **FFI.** An `extern` call puts the Nth argument in `XMM<N>` when it is a float (the integer register for that position is skipped, per Windows x64), spills beyond four as for ints, and reads a float result from `xmm0` into `rax`. Variadic calls also copy float arguments into the matching integer register, as the ABI requires.
10. **Interpreter.** `Value::Float(f64)` with the same rules; `f32` rounds through `as f32`. Printing uses Rust's shortest formatting adjusted to the same exponent thresholds, so text matches native output.
11. **Bare-metal.** The OS target does not enable SSE (`CR0`/`CR4` setup), so type resolution and literals reject floats when compiling for it.
12. **Assembler.** The hosted target is assembled by `ml64`, which already knows SSE2, so the codegen only needs MASM-compatible text (hex constants as `8000000000000000h`). The built-in assembler (`asm_x86.rs`, used for the bare-metal image) also gained the SSE2 forms, with byte-level tests, but the bare-metal target rejects floats, so nothing emits them there yet.

    Original note: Add the SSE2 forms listed in the proposal, handling the `F2`/`F3`/`66` mandatory prefixes and REX.W for 64-bit integer conversions. Each form gets a unit test comparing emitted bytes with known encodings.

## Risks / Trade-offs

- **Printing correctness** is the hardest part. Mitigation: compare native output with the interpreter (Rust's formatter) over many varied bit patterns, plus the fixed cases from the spec.
- **Assembler encodings** are easy to get subtly wrong. Mitigation: byte-level unit tests per instruction before codegen uses them.
- **XMM registers are caller-saved** and floats live in `rax`/slots between operations, so nothing is held in XMM across calls. Mitigation: call tests with floats in all four argument positions.
- **Slower than necessary** because of `movq` round trips. Accepted; the optimizer can keep values in XMM registers later.
- **`int(x)` panics** rather than saturating. Chosen for honesty and consistency with other panics; a saturating form can be added later without breaking this.
