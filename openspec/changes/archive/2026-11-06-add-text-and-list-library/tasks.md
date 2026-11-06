## 1. Compiler groundwork

- [x] 1.1 Register `text` and `list` in `std_source`, and infer a generic result type from a lambda argument's body (`list.map(xs, func(n) n * 2 end)` gives `[]int`) as described in the design; verify with generic-function tests that map to a different type and that fall back to a clear error when the result cannot be inferred

## 2. The text module

- [x] 2.1 Write `std/text.br` searching (`index_of`, `contains`, `starts_with`, `ends_with`); verify with the found, missing and empty-part scenarios
- [x] 2.2 Add `split`, `join`, `lines` and the balanced concatenation helper; verify the split, join and lines scenarios, the empty-separator panic, and joining 100000 pieces in reasonable time
- [x] 2.3 Add `trim`, `trim_start`, `trim_end`, `replace`, `repeat`, `pad_left` and `pad_right`; verify each scenario of the spec
- [x] 2.4 Add `upper`, `lower` and `reverse` (ASCII only, bytes intact); verify with the `Straße` scenario
- [x] 2.5 Add `parse_int` with overflow detection and error messages; verify valid, invalid, empty, overflow and `-9223372036854775808` inputs and `??`
- [x] 2.6 Move the big-number code out of `std/float.br` into an internal module shared with `text`, and add `parse_float` (exact fast path, correctly rounded slow path, `inf`, `NaN`); verify against Rust's parser on thousands of random decimal strings and bit patterns, and the print-then-parse round trip
- [x] 2.7 Verify no leaks: `live_objects()` stays flat over 1000 rounds of `split`, `join`, `replace` and `trim`

## 3. The list module

- [x] 3.1 Write `std/list.br` with `map`, `filter`, `reduce`, `any`, `all` and `find`; verify the spec scenarios including that `any` stops early
- [x] 3.2 Add `reverse`, `slice`, `concat` and `range`; verify clamping and empty cases
- [x] 3.3 Add `sort_by` (stable merge sort); verify numbers, texts, stability with records, and 100000 random integers
- [x] 3.4 Verify no leaks: `live_objects()` stays flat over 500 rounds of map, filter, sort and reverse on lists of texts and records

## 4. Interpreter

- [x] 4.1 Add a minimal module mechanism to the interpreter (`use text`, `use list`, calls `text.trim(...)`) and anything the std sources need from it; verify that `examples/words.br` prints the same output natively and with `--run`

## 5. Documentation and examples

- [x] 5.1 Add `examples/words.br` (word frequencies from a text, sorted) and its section on `docs/examples.html` (via `brasa-parse --docs`); verify it builds and prints the documented output
- [x] 5.2 Add `docs/stdlib.html` documenting both modules, link it from the navigation of every page, and update the status table and roadmap in `README.md`; verify `cargo test` passes, including the docs tests (shared navigation, links, code samples)
