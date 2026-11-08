## Context

`int_type` maps `"i64" | "isize" | "int"` to 64 bits; integer literals get `Ty::i64()`; `Ty::show` prints `i64`. `zila.ebnf` says literals default to i32. `c_type` (the `c.int`/`c.long` table for C types) already uses 32-bit `int` and `long` and is a separate namespace.

## Decisions

- **Aliases, not types.** `int_type` resolves `int` to `Ty::Int { bits: 32, signed: true }` and `long` to `{ 64, true }`: no new `Ty` variant, so nothing else in the compiler changes. `float` already resolves to `Ty::Float { bits: 64 }` the same way.
- **Display `int`/`long`.** `Ty::show` prints `int` for i32 and `long` for i64. Everyday code and the docs use those names, and the floating-point spec already promises "`int` was found". Since aliases are the same type, the compiler cannot know which spelling was written; printing the everyday one is the least surprising. Other widths keep `i8`, `u16`, and so on.
- **No name clash.** Conversions are already "type name applied like a call" (`i16(x)`). `int(x)` and `long(x)` go through the same path; `float(x)` stays special-cased for float. `c.int`/`c.long` stay in the C namespace and keep C sizes.
- **32-bit `int` is deliberate.** It matches C `int` and halves list/record memory. The pain of a narrow default is removed by three rules instead of by making `int` wide:
  1. **Lossless widening is implicit.** `int` flows into `long` (and `u32` into `i64`, and so on) wherever a value of the wider type is expected, and mixed arithmetic takes the wider type. Only value-preserving conversions are implicit; narrowing, signed/unsigned of the same width and integer/float stay explicit with `int(x)`, `long(x)` and `float(x)`. This is what keeps the migration small: almost every place that passed a count or offset to a `long` parameter keeps compiling untouched. The old rule "no implicit numeric coercion" in `zila.ebnf` is narrowed to "no implicit lossy coercion".
  2. **Lengths and indices are `int`.** `len`, counts and list indices stay `int`, so `for i in 0..xs.len` and `xs[i]` need no conversions and no `usize` spreads through user code. The cost is a documented ceiling of 2,147,483,647 elements (bytes for text) per collection, enforced by the runtime stopping the program (fatal failure) when an operation would exceed it. A collection that size needs 2 GiB even for bytes, so the ceiling matters only to programs that should be using chunked I/O anyway. `usize` remains for pointer arithmetic and C interop.
  3. **Sizes of the outside world are `long`.** File sizes and offsets, timestamps, byte counters, handles and hashes are `long` (or `usize` for addresses), decided library by library in task 2.1.
- **Arithmetic stays in the operand type.** `int * int` is computed and wraps in 32 bits even if the result is stored in a `long`; the compiler does not infer a wider operation from where the value ends up, because that would make a program's result depend on distant code. `long(a) * long(b)` is the explicit exact form, and `checked_mul` (see `remove-panic-add-abort`) reports overflow.
- **Generic inference and interfaces get no widening.** `max<T>(a: int, b: long)` and interface method signatures need exact types. Implicit widening there would make type inference ambiguous and one type satisfy an interface in two ways. The error asks for an explicit conversion.
- **Overflow** behavior of arithmetic is unchanged by this change; it simply happens at 32 bits for `int`.

## Risks

- Rewrite of code that silently relied on 64-bit `int` (timestamps, `fs` sizes, hashes, handles, pointers cast to `int`). Mitigation: widening makes most call sites keep compiling, so the rewrite is limited to places where an `int` was a 64-bit accumulator (sums, products, hashes, time arithmetic), which are found by review rather than by compiler errors. A full test run after each library, and the existing literal range checks, backstop this.
- Silent 32-bit wrap in an `int` accumulator that used to be 64-bit. Mitigation: task 2.1 lists every accumulator in `std/` (hash functions, `format` with thousands, time and size arithmetic) and a test per library that exceeds 2^31.
- The 2 GiB ceiling on collections. Mitigation: documented, enforced, and `fs` reads larger than that fail with an error value instead of reaching it.

## Depends on

- `remove-interpreter`: `src/interp.rs` is not updated here and goes away there.
- `add-direct-pe-output`: independent; `os/` rewrites must keep its tests passing.
- Coordinates with `add-text-bytes-and-char`, `add-error-kind`, `add-json` and `add-package-manager`: they should write `int`/`long` per this change; no scope is duplicated.
