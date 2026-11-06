## Why

Programs that print tables, reports or money amounts still build their text by hand with `+` and `math.fixed`, and programs that need "have I seen this before" use a `Map<K, bool>` and ignore the values. A formatting module and a set type are the next everyday pieces of the standard library, and both can be written in Brasa on top of what already exists (text, lists, maps, generics).

## What Changes

- New library module `format` (`use format`):
  - `format.text(template, args)` replaces `%` and `%0`-style placeholders with the texts in `args` (a `[]text`), with an optional layout in parentheses: fill, alignment and width (`%(>8)`, `%(<8)`, `%(^8)`, `%(*^9)`, `%(08)`), and `%%` for a literal percent sign. Placeholders do not use braces because a brace in a text literal starts an interpolation.
  - Helpers that turn values into text for those arguments: `format.int(n)`, `format.hex(n)`, `format.bin(n)`, `format.float(x, digits)`, `format.thousands(n)`.
  - Padding helpers: `format.pad_left`, `format.pad_right`, `format.center`.
  - A template that does not match its arguments is reported, never silently ignored.
- New library module `set` (`use set`): a generic `Set<T>` for `text` and integer elements, written in Brasa over `Map<T, bool>`: `set.empty()`, `add`, `has`, `remove`, `clear`, `len`, `to_list` for iterating (`for x in s.to_list()`; looping over a user type directly is not part of this change), and `union`, `intersect`, `difference`, `is_subset`, `from_list`.
- The interpreter gets the same modules (it already loads library modules).
- Out of scope, kept on the roadmap: buffered I/O, UDP, TLS, async DNS, sorted maps and sets, and placeholders that format values of any type directly (that needs a way to pass values of mixed types to one function).

## Capabilities

### New Capabilities

- `format-library`: the `format` module (templates, layout, number helpers, padding).
- `set-library`: the `set` module (the `Set<T>` type and its operations).

### Modified Capabilities

None.

## Impact

- New files `std/format.br` and `std/set.br`, registered in `std_source` in `src/codegen_x64.rs` and in the interpreter's module loader.
- Possible small compiler fixes if generic code over `Map<T, ...>` or iteration over a user type needs them (found while implementing; any such fix is listed in the design).
- `docs/stdlib.html`, `README.md`, an example `examples/report.br`, and tests `tests/format.rs` and `tests/sets.rs`.
