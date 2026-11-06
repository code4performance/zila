## Context

The standard library modules `text`, `list`, `float` and `math` are written in Brasa, embedded with `include_str!` through `std_source` and loaded when a file says `use name`. The interpreter loads only `text`, `list`, `float` and `math` (a name list in `interp.rs`). `math.fixed(x, digits)` already writes a float with a fixed number of decimals, and `text` has `split`, `join`, `replace`, `parse_int` and friends. Generics (monomorphization, with `Map<K, V>` keys limited to `text` and integers) and `Map<K, V>` with insertion-ordered iteration exist. There is no value that can hold "any type", so a function cannot take a mixed list of arguments; user types cannot be iterated with `for`.

## Goals / Non-Goals

**Goals**
- A `format` module that covers the everyday layout needs (padding, alignment, width, number bases, fixed decimals, thousands separators) and a template function, all in Brasa.
- A generic `Set<T>` over text and integers with the usual operations, in Brasa, with a deterministic order.
- The same results natively and in the interpreter.

**Non-Goals**
- Placeholders that format any value by type (`{}` with an int and a float in one call). That needs variadic or `any` arguments; the template takes a `[]text` and the `int`, `float`, `hex`, ... helpers convert values.
- Locale-aware formatting, date and time formatting, `printf`-style `%` codes.
- Sorted sets and maps, and `for x in set` (needs an iteration protocol).

## Decisions

1. **`format.text(template, args: []text) -> text or error`.** A single left-to-right scan over the template bytes. `%%` is a literal percent; `%` otherwise starts a placeholder: optional index digits, then optionally `(layout)` up to the next `)`. A bare `%` takes the next automatic index; mixing automatic and explicit indexes is allowed (automatic ones count only the automatic placeholders). A layout is `[[fill]align][0][width]`: `align` is `<`, `>` or `^`; a fill is any one character before an alignment; `0` before the width means fill `0` with right alignment. Errors name what is wrong (`missing argument 2`, `unclosed '(' in placeholder`, `bad layout 'x'`). **Why not `{}`:** a brace inside a text literal starts an interpolation (`"{} is {}"` does not parse; a literal brace is `\{`), so `%` was chosen over `{}` and `$`, with the layout in parentheses so that its end is clear (`%1(>8)` then text).
2. **Width counts characters.** A helper `chars(s)` counts bytes that are not UTF-8 continuation bytes (`(b & 0xC0) != 0x80`), so `é` has width 1. Padding is built by repeating the fill. A multi-byte fill character is allowed (it is copied whole).
3. **Number helpers** reuse existing code: `int(n)` is `"{n}"`; `float(x, digits)` calls `math.fixed` (and so the `format` module does `use math`; if `math.fixed` mishandles negative halves, it is fixed there, since the spec requires half away from zero); `hex` and `bin` convert with repeated division on the magnitude, handling the smallest 64-bit integer without negating it (work with the negative remainder instead); `thousands` inserts commas into the decimal digits the same way.
4. **`set` representation.** `type Set<T> = record items: Map<T, bool> end` with methods defined as generic methods. `Map` preserves insertion order and `remove` leaves later entries in order, which gives the `to_list` order the spec promises with no extra bookkeeping. `add` returns whether the key was new by checking `has` first.
5. **Element types.** A generic record declares `Map<T, bool>`; the compiler accepts that in the template and checks the key type when the type argument is known, so no compiler change is needed. The error for `set.Set<Point>` is the existing map key error (`Point cannot be a map key; keys are text or integers`).
6. **`set.empty()`** (not `set.new()`: `new` is a reserved word). Types come from the expected type, like `{}` literals (`let a: set.Set<text> = set.empty()`; types from a module are written with the module name, as for `net.Listener`); without one the compiler asks for a type annotation (the generic inference already reports this for functions whose type argument cannot be inferred). `from_list(xs)` infers `T` from the list.
7. **Operations** are plain loops over `to_list()` of the smaller or left operand; `union` copies `a` then adds `b`, `intersect` keeps elements of `a` that are in `b`, `difference` keeps elements of `a` that are not in `b`. Results keep `a`'s order first. They allocate new sets and never touch their arguments.
8. **Interpreter.** `format` and `set` are added to the loadable names in `interp.rs`; the interpreter is dynamically typed, so `Set` works as long as generic records and `Map` methods already work there (checked by the shared example test). If the interpreter lacks a needed feature (for example `{}` literals typed through an annotation), the module avoids it, or the gap is closed in the interpreter and noted here.
9. **Docs and example.** `docs/stdlib.html` gets two sections; `examples/report.br` prints a table and `examples/sets.br` a short word-uniqueness program; the existing docs tests that require every library module and flag to be documented will cover the new modules.

## Risks / Trade-offs

- **Texts for arguments are clumsy** (`format.text("{} costs {}", [name, format.float(p, 2)])`). Accepted: it is the limit of the type system today, and the helpers keep it short; value-typed placeholders can come with variadic generics later without changing the template syntax.
- **`math.fixed` correctness for edge cases** (negative halves, large magnitudes, `-0.0`) becomes a format requirement. Mitigation: tests for the listed cases; fix in `math` if needed.
- **Generic record over `Map<T, bool>`** may hit compiler limits not seen yet (generic record fields of generic type). Mitigation: found first, in the first set test; the fallback is to write `Set` for text and int through two small non-generic records, at the cost of the spec's single generic type, and the spec would be revised before archiving.
- **Interpreter gaps** (lambdas don't capture locals, no `match` on results) may force the library to avoid some constructs; both modules use loops and plain functions only.
