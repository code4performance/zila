## 1. Groundwork

- [x] 1.1 Spike once `add-comptime-reflection` has landed: a throwaway test decodes `{"a":1,"b":"x"}` into a two-field record through the reflection API only; verify it passes and write down any gap found against that change (no compiler code is added in this change)
- [x] 1.2 Create `std/json.br` (`package json`) with the private scanner (peek, next, skip whitespace, expect, the 128 depth counter) and register `"json" => Some(include_str!("../std/json.br"))` in `std_source` in `src/codegen_x64.rs`; verify with a test that `use json` compiles for the Windows target and for the Linux target

## 2. Dynamic values

- [x] 2.1 Define `json.Value` and `parse_value` for null, bool, strict numbers (keeping the literal), lists and objects with position-carrying errors of kind `InvalidInput`; verify the tree, strict grammar (`[1,]`, `01`, `NaN`, empty text) and large-integer scenarios
- [x] 2.2 Add string decoding: escapes, `\uXXXX`, surrogate pairs, control-character and UTF-8 validation; verify the unicode, lone surrogate and invalid byte scenarios
- [x] 2.3 Add `stringify` and `stringify_with` (`Options{ indent }`, `\n` line ends, empty containers) writing through a list of pieces; verify the compact, indented, literal-preserving and duplicate-key (last wins) scenarios, and that stringifying 100000 items finishes in reasonable time
- [x] 2.4 Verify the depth limit in `parse_value` and `stringify`: 128 levels succeed, 200 fail with a message naming the limit, with no stack overflow

## 3. Typed decode

- [x] 3.1 Implement `parse(T, s)` for `bool`, `text`, integer types (exact range checks, integral exponent forms) and floats (via `text.parse_float`); verify the overflow, `u8`, `-9223372036854775808`, `1e2`, `5.0`, `1.5` scenarios
- [x] 3.2 Add lists, `Map<text, V>` and optionals; verify the nested list and map, and optional and missing scenarios
- [x] 3.3 Add records with unknown-key skipping, missing and duplicate field detection, and `json.Value` as a target; verify the record, missing required key, unknown keys and duplicate scenarios
- [x] 3.4 Add enums (payload-free as text, with payloads as one-key objects); verify the enum scenario
- [x] 3.5 Add error paths (`$.users[2].email`) everywhere; verify the syntax-position and type-path scenarios, and a fuzz test: 2000 random byte mutations of valid documents all return a value or an error and never stop the program

## 4. Typed encode and attributes

- [x] 4.1 Implement `encode` and `encode_with` for all supported types, errors for NaN and infinities, and string escaping; verify the record-out, escape and not-finite scenarios, and the round trip over a nested record, list, map and optional
- [x] 4.2 Implement `#[json("key")]` and `#[json(skip)]` in both directions with comptime errors for bad attribute content and key clashes; verify the rename, skip, bad attribute and key clash scenarios using the `User` type of `examples/types.br`
- [x] 4.3 Verify a user type declared in a test with no other registration decodes and encodes, and search the compiler sources (`src/`) for the word `json`: nothing may match except the `std_source` line

## 5. Quality

- [x] 5.1 Verify no leaks: `live_objects()` stays flat over 1000 rounds of decode and encode of a record holding texts, lists and maps
- [x] 5.2 Verify float fidelity: encode then decode thousands of random bit patterns and compare the bits
- [x] 5.3 Verify identical output on the Windows and Linux targets (the same program built for each in `tests/json.rs`; the Linux run is skipped where there is no Linux runner, as the other Linux tests do)
- [x] 5.4 Measure compile time and executable size of a program decoding five different types and record the numbers in design.md under "Changes made while building"

## 6. Examples, docs, tests

- [x] 6.1 Write `tests/json.rs` covering every scenario of the spec (skipping native runs when the toolchain is missing, like `tests/stdlib.rs`); verify `cargo test --test json` passes
- [x] 6.2 Check `examples/errors.br` and `examples/types.br` still parse and, where the program is complete, compile with the final `json.parse` signature; adjust the examples if the signature differs; verify with `cargo test --test parse`
- [x] 6.3 Add `examples/json.br` (read a config text with a rename and an optional field, print it pretty) and its section in `docs/examples.html` via `zila-parse --docs`; verify it builds and prints the documented output
- [x] 6.4 Add a `json` section to `docs/stdlib.html` (functions, mapping table, attributes, errors, limits) and update the status table and roadmap in `README.md`; verify `cargo test --test docs` passes (navigation, links, code samples) and `cargo test` passes (leaving out `tests/async.rs`, per the routine)
