## Why

Today a Zila Lang program can only measure a text (`len`), cut it (`slice`), read a byte (`s[i]`) and join two (`+`); lists only `add`, `take`, `clear` and index. Anything real (reading a number a user typed, splitting a line into words, trimming input, sorting a list, mapping over values) has to be hand-written each time. Generics and floats are now in, so the standard library can finally grow. It is item 1 on the roadmap ("Standard library: text, collections").

## What Changes

- **`use text`**: functions on text, written in Zila Lang: `split(s, sep)`, `join(parts, sep)`, `trim(s)`, `trim_start`, `trim_end`, `contains(s, part)`, `starts_with`, `ends_with`, `index_of(s, part)` (-1 when absent), `replace(s, from, to)`, `repeat(s, n)`, `upper(s)`, `lower(s)`, `lines(s)`, `reverse(s)`.
- **Parsing and formatting**: `text.parse_int(s)` and `text.parse_float(s)` return `int or error` and `float or error` (so `try` and `??` work); `text.pad_left(s, width, fill)` and `text.pad_right`.
- **`use list`**: generic helpers over lists: `map(xs, f)`, `filter(xs, keep)`, `reduce(xs, start, f)`, `any(xs, test)`, `all(xs, test)`, `find(xs, test)` (index or -1), `reverse(xs)`, `slice(xs, from, to)`, `concat(a, b)`, `sort_by(xs, less)` (stable merge sort), and `range(a, b)` for `[]int`.
- **Method syntax is not added**: calls are `text.trim(s)` and `list.map(xs, f)`. (Methods on built-in types already work through receivers, but the library stays in modules so it never shadows a user's own functions.)
- **Interpreter**: `--run` provides the same modules so programs that use them run the same (the modules are Zila Lang source, the interpreter runs them as written).
- Docs, `examples/words.br`, tests, and the README status table and roadmap are updated in the same change.

## Capabilities

### New Capabilities
- `text-library`: the `text` module: searching, splitting, joining, trimming, replacing, case, padding, parsing numbers.
- `list-library`: the `list` module: generic transforms, searching, ordering and construction.

### Modified Capabilities
<!-- None: no existing behavior changes. -->

## Impact

- **Standard library** (new): `std/text.br`, `std/list.br`, registered in `std_source` in `src/codegen_x64.rs` so `use text` and `use list` work.
- **Compiler**: only what the library cannot express in Zila Lang; expected to be small (checked task by task, anything needed is surfaced before adding it).
- **Interpreter** (`src/interp.rs`): load the same module sources for `use text` / `use list`; the interpreter has no modules today, so this needs a small module mechanism (qualified calls `text.trim(...)` to functions of the loaded file).
- **Docs, examples, tests**: new `docs/stdlib.html` page (linked from every page's navigation), `docs/examples.html`, `README.md`, `examples/words.br`, new `tests/stdlib.rs`.
- **Assumptions** (no clarification needed):
  - Text is bytes (UTF-8). Searching, splitting, trimming and replacing work on bytes and are correct for UTF-8 because they match whole byte sequences; `upper`, `lower` and `reverse` only change ASCII letters and reverse bytes, and say so in the docs.
  - `trim` removes space, tab, carriage return and newline.
  - `list` functions never change their input; they return new lists. `sort_by` takes a `less` function because `T` has no built-in ordering; a `Comparable` interface is a later step.
  - Element ownership follows the existing list rules, so lists of texts and records are retained correctly and nothing leaks.
  - Not included: regular expressions, Unicode case mapping and grapheme handling, `format` with placeholders, hash sets, sorted maps, buffered I/O, UDP, TLS and async DNS (the rest of the roadmap item).
