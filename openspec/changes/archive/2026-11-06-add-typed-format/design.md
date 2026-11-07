## Context

`std/format.br` has `text(template: text, args: []text) -> text or error`, built on `apply_layout(s, layout)` (which parses `[[fill]align][0][width]` and pads), plus helpers (`int`, `hex`, `bin`, `thousands`, `float(x, digits)`, `pad_*`). A probe showed that, natively: an `enum` with data (`Int(int)`, `Float(float)`, `Text(text)`, `Bool(bool)`), an interface with one method (`to_value(self) -> Value`), methods declared on the built-in types `int`, `float`, `text` and `bool`, a method on a user record, and a generic function with interface bounds (`func two<A: Valued, B: Valued>(a: A, b: B)`) all work together: `a.to_value()` inside the generic body resolves per instance. The interpreter cannot construct enum variants with data (`undefined identifier 'Int'`).

## Goals / Non-Goals

**Goals**
- `format.fmt("...", a, b)` with values of any printable type, and number formats in the placeholder, with no change to the language beyond a tiny rewrite.
- User types can join in by defining `to_value`.

**Non-Goals**
- Variadic heterogeneous arguments in the language, a `Printable`/`Display` mechanism for `print` and interpolation, locale-aware numbers, an `e` (scientific) float format, interpreter support for payload enums.

## Decisions

1. **`Value` and `Valued` in `format`.** `type Value = enum Int(int), Float(float), Text(text), Bool(bool) end` and `type Valued = interface func to_value(self) -> Value end`. `format` declares `to_value` for `int`, `float`, `text` and `bool`; a user type satisfies `Valued` structurally by declaring the method (its result is usually `format.Text("...")`).
2. **One core, many entry points.** `values(template, args: []Value) -> text or error` is the template engine (the existing `text` code, moved: placeholders `%`, `%n`, `%%`, `%(layout)`, the same error messages). The interpreter cannot build `Value`s, so `text(template, args: []text)` does not go through them: both `text` and `values` share `parse_template(template, nargs)`, which cuts the template into literal and placeholder pieces (and reports the missing-argument and unclosed-`(` errors at the same points as before), and they differ only in how a placeholder is rendered (`apply_layout` for texts, `apply_value` for values). The layout is parsed by one function (`parse_layout`, into a `Spec` record) and padded by one function (`pad`) for both, so the behaviour of `text` is unchanged. `fmt0` to `fmt8` are generic functions with one type parameter per value, each bounded by `Valued`: `func fmt2<A: Valued, B: Valued>(template: text, a: A, b: B) -> text or error return values(template, [a.to_value(), b.to_value()])`.
3. **`format.fmt` is a name for `fmtN`.** In the call compiler, a call whose callee is `format.fmt` (the module's own name, only when the program imports `format`, and when the program has no function of that name) is compiled as a call of `format.fmtN` with N the number of arguments after the template; N greater than 8 is the error `format.fmt takes at most eight values`. This is one rename at the place where calls to `module.function` are resolved.
4. **Typed layouts.** The layout parser reads `[[fill]align][0][width]` as before, then an optional `.` and digits (precision) and an optional type: one of `d x X b , f s`. `apply_value(v, layout)` first renders the value (the type decides how; no type: the `print` form, which for floats is the library's `float.to_text`, the shortest form), then pads the text like `apply_layout`. Rendering reuses `to_base`, `thousands` and `float(x, digits)` (already correct for halves away from zero, negatives and the extreme integers); `X` upper-cases `x`'s result; `,` accepts only integers.
5. **Errors.** An unknown letter or malformed layout gives `bad layout '<layout>'` (as now). A letter that does not fit gives `'x' needs an integer, got text` (kinds: `integer`, `float`, `text`, `boolean`); a precision on a non-float gives `a precision needs a float, got integer`; they name the letter and the kind of the value.
6. **Layout parsing is not duplicated.** `apply_layout(s, layout)` for texts stays as the entry used by `text` (so its behaviour cannot change); the core calls a shared helper that returns the parsed pieces (fill, align, zero, width, precision, type) and both paths pad with the same function. For a `Text` value a type letter other than `s` is the mismatch error.
7. **Interpreter.** Nothing is added. `use format` keeps loading (the enum and the interface parse fine); only calling the typed functions fails there, and the docs say `format.fmt` is native only.
8. **Tests.** Native tests for each scenario (mixed values, indexes, the user type, every type letter, width and fill with types, float precision and rounding, the mismatch and unknown-letter errors, nine values rejected, eight accepted), a regression run of the existing format tests (unchanged expectations), and the interpreter test for `format.text` and the helpers.

## Risks / Trade-offs

- Arity-specific functions (`fmt0` to `fmt8`) are repetitive; the rename keeps them out of user code. Eight values cover real use; the list form `format.values(template, [..])` accepts any number for those who build the values themselves.
- Each `fmtN` instance is monomorphized per combination of argument types, which adds a little code per distinct call shape.
- A user type with a `to_value` of the wrong result type fails to satisfy `Valued`, and the compiler's bound error is what the user sees.
