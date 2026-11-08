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

## Changes made while building

Recorded after the fact. This change was written before `add-int-long-aliases`, `add-derived-eq-ord-hash`, `add-copy-on-write`, `add-text-bytes-and-char`, `add-weak-refs`, `add-threads-and-shared`, `add-comptime-reflection` and `add-visibility-pub` landed; all of them had landed when it was applied, and it was reconciled with them as follows.

**Reconciliation with the changes that landed first**
- `pub` (decision 1): the public surface is the eight names of the design. The generated code is compiled in the program that asked for it (below), so it can only call public functions of `json`; those are named with a `__` prefix (`json.__open`, `json.__text`, ...) and documented as not for use. `State` and `Out` are public records with private fields, so programs cannot build or poke them.
- `int` and `long`: integer targets are read with one function, `__int(st, bits, signed, name)`, that returns a `long` (an unsigned 64-bit value comes back as its bit pattern); the generated code converts to the exact type (`i8(...)`, `int(...)`, `u16(...)`). `Options.indent` is an `int`. No arithmetic goes through floats.
- `char` and bytes (decision 7): the scanner works on `s.byte(i)` and `s.slice(a, b)`; only `\uXXXX` goes through `char(n).to_text()`. `char(n)` aborts on a surrogate, so the surrogate rules are checked before calling it.
- Copy on write: the reader and the writer are records passed with `ref` to every function (a `ref` parameter cannot be passed on by value, so even the read-only helpers take `ref`), so nothing is copied while a document is read or written.
- Derived eq/ord/hash: nothing needed; `Map<text, V>` is the object type and keeps insertion order.
- Comptime derive: not used. The design asks for the mapping to be derived by a generated function (`#[generated]`), not a `#[derive]`, so that `json.parse(T, s)` needs no attribute on `T`.
- Rc, Weak and threads: not part of JSON. A type holding `Rc`, `Weak`, `Future` or `Mutex` is refused when compiling, as are functions, sets and pointers, with the field named.

**Decisions that changed**
- **`encode` takes the type**: `json.encode(T, x)` and `json.encode_with(T, x, opts)`, or `json.encode<T>(x)`. The generator that writes the encoder sees types, not values, and the compiler expands generated calls before it knows the type of an expression, so `json.encode(x)` cannot work. The spec was adjusted. Generic types (`Map<text, int>`, `[]int`) can only be written as a type argument, `json.parse<Map<text, int>>(s)`.
- **Direct decoding with a path stack**: as decision 2 says, the decoder drives the scanner and builds the `T` directly. Duplicate keys are found with a flag per field; unknown keys are skipped with a syntax and depth check. The path for error messages is a stack of keys and indexes in two lists with a height (lists have no `pop`), rendered only when an error is made.
- **Generated helpers**: per type there is one specialization of `__dec` and one of `__enc`. The leaf types (numbers, `bool`, `text`, `json.Value`) are written inline into their parents so they cost no specialization and no generation round.
- **Unit and payload cases in one enum**: an enum that has both payload-free cases and cases with payloads reads each case in its own form (a text for the former, a one-key object for the latter). The spec only described the two pure cases.
- **Optional fields**: the compiler only allows `T?` for reference types (text, lists, maps, records), so `int?` fields cannot be declared and the library does not need to handle them.
- **Zero values for `#[json(skip)]`**: records get a record of zero values, an enum its first payload-free case (or the first case with zero payloads), a `Value` gets `Null`. A type with no zero value (a recursion deeper than six levels) is refused when compiling.
- **Dynamic `Number`**: a hand-built `Number(f, "")` has no literal and is written from its float; a literal that is not a JSON number is also ignored.
- **Duplicate keys inside ignored members** are not detected (they are only checked for syntax); the spec text was narrowed to the keys the target uses.

**Gaps found in `add-comptime-reflection` (task 1.1, fixed here because nothing else can)** — the compiler gained nothing about JSON, but four general defects of the reflection work had to be fixed:
1. A specialization of a `#[generated]` function declared in an imported package was compiled in that package, where the program's own types are invisible (private to `main`), and the generated calls it contained were never expanded. It is now compiled in the module of the caller (its signature types are written `json.State`, and it is never `pub`), which sees the caller's private types and fields and has its nested calls expanded in the next round. Recursive types work because specializations are keyed by type. Consequence: types nested deeper than about six levels in one call hit the limit of 8 rounds.
2. Attribute arguments were plain text, so `#[json(rename)]` and `#[json("rename")]` were the same. `reflect.Attribute` gained `quoted: []bool` (and the wire format a flag token per attribute) so a generator can tell a bare word from a quoted text.
3. `Type.element()` was missing `pub`, so no library could ask the element type of a list.
4. The error message of an unknown derive named `json.json` as its example; it is `pkg.describe` now, so that the compiler sources have no mention of JSON except the line that registers the library.

**Measurements (task 5.4)**, on the debug build of the compiler: a program that decodes `Address`, `User` (with a rename, an optional and a list of `Address`), `Shape` (an enum with payloads), `Settings` (a map, a list of enums and a float) and `Team` (a list of `User`, a `User` and `Settings`) and encodes a `Team` compiles in 4.5 s and gives a 130,560-byte executable; `hello.br` takes 0.43 s and gives 97,280 bytes. The 33 KB difference is the library plus nine specializations. The generator results are cached, so a rebuild does not run them again.
