## Context

The standard library is Brasa source embedded in the compiler (`std_source` in `src/codegen_x64.rs`: `text`, `list`, `format`, `float`, ...). `std/float.br` already prints the shortest round-trip digits (`to_text`) and `std/text.br` has `parse_int` and `parse_float`. Records can carry field attributes (`#[json("full_name")]` parses today, see `examples/types.br`) and `examples/errors.br` calls `json.parse(Config, raw)`, but no `json` module exists. What a library needs and the language lacks today is looking at a type: its fields, their names and attributes, the cases of an enum, and constructing values of it. That is the job of `add-comptime-reflection`.

**Depends on**
- `add-comptime-reflection`: type arguments as values (`json.parse(T, s)`), enumerating the fields, attributes, cases and element types of `T`, and building a `T` generically. The whole derivation lives on it.
- `add-error-kind`: the `kind` of an error value (`InvalidInput`).
- `add-text-bytes-and-char`: the `char` type and byte access to text that the scanner, the `\uXXXX` handling and the UTF-8 validation use.
- `add-visibility-pub`: only `parse`, `parse_value`, `stringify`, `encode`, the `*_with` variants, `Value` and `Options` are `pub`; the scanner and derivation helpers stay private.
- Related, not required: `add-direct-pe-output` (changes how Windows executables are produced, not what the library needs); `add-int-long-aliases` (only the spelling of integer types in docs and tests).

## Goals / Non-Goals

**Goals**
- JSON entirely in Brasa, derived by reflection, with no JSON code in the compiler.
- Errors that say where the problem is.
- Safe against hostile input: no stack exhaustion, no silent number damage.

**Non-Goals**
- Streaming, JSON Pointer, Schema, comments, trailing commas.
- Custom encoders and decoders per type (a follow-up; the derivation is written so a user function could later be consulted first).

## Decisions

1. **One module, `std/json.br`, `package json`.** Public surface: `Value`, `Options`, `parse_value`, `stringify`, `stringify_with`, `parse`, `encode`, `encode_with`. Everything else is private. No dependency on `net`, `fs` or any OS module, so the Linux and Windows targets share the file (no `json.linux.br`).
2. **Scanner shared by both decoders.** A private `Scan` record (the text, a byte offset, depth) with `peek`, `next`, `skip_space`, `expect`. `parse_value` builds a `Value` tree; `parse(T, s)` drives the same scanner but builds a `T` directly (no intermediate tree) so large documents are not held twice. Line and column are computed from the offset only when an error is created, by counting `\n` and characters up to the offset; the success path pays nothing.
3. **Decode derivation by reflection.** `parse` calls a private generic decoder that switches on the kind of `T` (record, enum, list, map, optional, int, float, bool, text, `Value`) and recurses with the element or field type. The record case enumerates fields, applies attributes, and records which keys were seen to report duplicates and missing fields. Unknown keys are skipped with a private `skip_value` that validates syntax and depth. Because the switch is on a comptime value, each instantiation contains only the code for its own type.
4. **Encode derivation** is the mirror: a private encoder writing into a list of text pieces that `encode` concatenates once (the balanced concatenation approach of `std/text.br`), avoiding quadratic `+` in loops.
5. **Attributes.** `json("key")` and `json(skip)` are read through reflection. Validation (bad attribute content, key clashes) runs at comptime so mistakes are compile errors naming the field, which requires reflection to be able to raise a comptime error with a message. If it cannot, the task records it against that change and the two checks become decode-time errors, documented.
6. **Numbers.** The scanner validates the RFC grammar and keeps the literal's byte range. For integer targets it parses the literal exactly: digits accumulated negatively with an overflow check at the target's range (`u8`, `i64` and `-9223372036854775808` come out right with no float detour); a literal with fraction or exponent is accepted only when its value is integral (digits are shifted by the exponent, never converted through a float). For float targets it hands the literal to `text.parse_float` (correctly rounded). `Value.Number` stores the float and the original literal, so `stringify` of an unmodified number prints the literal; numbers built by hand print the shortest text from `float.to_text`, adjusted into JSON grammar (`NaN` and infinities are errors, so `encode` and `stringify` return `text or error`).
7. **Text.** Decoding validates UTF-8 while scanning (reject overlong forms, surrogates, values above U+10FFFF), copies unescaped runs with `slice` and builds escapes through `char`. A high `\uXXXX` surrogate must be followed by a low one. Encoding copies runs that need no escape with `slice`.
8. **Errors.** A private helper makes the syntax message with line and column; another makes the type message. The path is carried as a list of segments (a key or an index) pushed and popped around recursion and rendered only on error, so success has no formatting cost. Both create `error` values with kind `InvalidInput`.
9. **Depth limit 128**, counted on entering a list or object, in the scanner, the decoder and the encoder (so a cyclic structure through shared records cannot loop forever). It is a private constant, documented.
10. **Duplicate keys.** Typed decode rejects them (silent last-wins hides bugs in configuration). The dynamic `Value` keeps the last value, which matches common parsers and keeps `parse_value` total over every grammatical document.
11. **Options.** `Options` is a record with `indent: int`; `encode` and `stringify` use the default. A record leaves room for future options (key sorting, escaping non-ASCII) without breaking callers.
12. **Examples.** `examples/errors.br` and `examples/types.br` already define the intended API and keep their text; a task verifies they still match the final signatures. A new `examples/json.br` is a complete, runnable program.

## Risks / Trade-offs

- **Reflection gaps.** If `add-comptime-reflection` cannot construct records field by field, decode needs another route. Mitigation: this change starts only after that one lands; the first task is a spike decoding a two-field record, and any gap is reported against that change instead of adding compiler code here.
- **Compile time and code size.** Each decoded type instantiates a copy of the derivation. Mitigation: shared non-generic helpers for the scanner, strings and numbers; only the structural switch is per type. Measured in a task.
- **Number fidelity** depends on `text.parse_float` and `float.to_text` being exact; both already have round-trip tests, and the JSON tests repeat them over many bit patterns.
- **Strict duplicate rejection** surprises users of lenient parsers. Documented, and `json.Value` is the escape hatch.
- **Integer-as-float.** A `Value.Number` is a float for arithmetic, which loses integers above 2^53; the literal text is kept so pass-through data is not damaged, at the cost of a second payload.
