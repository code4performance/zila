## Why

Brasa programs talk to the outside world (`examples/rest_api.br`, `examples/errors.br`, `std/ws.br`), and the common format on that boundary is JSON. Today there is no way to read or write it except by hand-parsing text. The repository's own examples already show the intended API (`json.parse(Config, raw)` returning `Config or error`, and `#[json("full_name")]`, `#[json(skip)]` and optional `text?` fields on `User`), but nothing implements it. With comptime reflection planned, JSON can be a plain library written in Brasa, with no compiler special case for it.

## What Changes

- **`use json`**: a standard library module in `std/json.br`, registered in `std_source` in `src/codegen_x64.rs`, working on the Windows and Linux targets.
- **Dynamic values**: the enum `json.Value` (`Null`, `Bool`, `Number`, `Text`, `List`, `Object`), `json.parse_value(text) -> Value or error` and `json.stringify(value) -> text or error`.
- **Typed decode**: `json.parse(T, text) -> T or error`, where `T` is passed as a type argument. Supported: records, enums, lists, `Map<text, V>`, optionals, integer types, floats, `bool`, `text` and `json.Value`.
- **Typed encode**: `json.encode(x) -> text or error` for the same set of types.
- **Derived through reflection**: the mapping from a type to JSON is derived by library code using the reflection of `add-comptime-reflection`; the compiler gains nothing JSON specific.
- **Field attributes**: `#[json("name")]` renames a field, `#[json(skip)]` leaves it out of both directions, an optional field (`text?`) maps to `null` or an absent key.
- **Errors**: values of `add-error-kind` with kind `InvalidInput`, and a message that carries the position of the problem (line and column for syntax errors, a path such as `$.users[2].email` for type errors).
- **Rules**: integer overflow is an error, floats print as the shortest text that reads back identically, duplicate keys are an error when decoding into a type and last-wins for `json.Value`, nesting depth is limited, and output can be pretty printed with `json.encode_with` and `json.stringify_with`.
- Docs, `examples/json.br`, tests, and a check that the examples that mention JSON match the real API.

## Capabilities

### New Capabilities
- `json-library`: the `json` module: dynamic values, typed decode and encode, attributes, text and number rules, errors, limits, pretty printing.

### Modified Capabilities
<!-- None: no existing spec changes behavior. -->

## Impact

- **Standard library** (new): `std/json.br`; registered in `std_source` (`src/codegen_x64.rs`). Uses `std/float.br` for shortest float text and `std/text.br` for number parsing.
- **Compiler**: none expected. Anything the library cannot express through reflection is a defect of `add-comptime-reflection` and is reported there, not patched here (recorded in design.md).
- **Examples**: `examples/errors.br` and `examples/types.br` already use the API; they SHALL keep working with the final signatures. New `examples/json.br`.
- **Docs and tests**: `docs/stdlib.html` (new `json` section), `docs/examples.html` (via `brasa-parse --docs`), `README.md` status table and roadmap, new `tests/json.rs`.
- **Assumptions** (no clarification needed):
  - Input and output text is UTF-8. Invalid UTF-8 in input is an error. A leading byte order mark is rejected.
  - Numbers decode into integer types when the target is an integer type and the literal is integral, into floats otherwise. `json.Value.Number` keeps the original literal text next to the float so large integers are not silently rounded.
  - Not included: streaming parsers, JSON Pointer, JSON Schema, comments or trailing commas (strict RFC 8259), custom per-type encoders (a later extension of the derivation), the HTTP side of JSON.
