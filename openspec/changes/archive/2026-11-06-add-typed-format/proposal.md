## Why

`format.text(template, args)` takes a list of texts, so a program that wants to put a number in a template has to turn each value into text first (`format.int(n)`, `format.float(x, 2)`, `"{b}"`), which defeats most of the point of a template. Numbers also want formatting of their own (hexadecimal, thousands separators, fixed decimals) inside the placeholder. This is the "formatting values of any type" item of the standard library roadmap.

## What Changes

- `format.fmt(template, a, b, ...)` takes up to eight values of any printable type (integers, floats, booleans, text) and returns `text or error` like `format.text`: `format.fmt("%(<8)|%(>8.2f)", name, price)`. A value of your own type can be formatted too, by giving it a method `to_value()` that returns a `format.Value` (usually `format.Text(...)`).
- Placeholders get a type and a precision at the end of the layout: `[[fill]align][0][width][.precision][type]`, with the types `d` (decimal), `x` and `X` (hexadecimal), `b` (binary), `,` (thousands separators), `f` (fixed decimals, from the precision, 6 by default) and `s` (text). `%(08x)` is a zero-filled hexadecimal number of width 8, `%(>10,)` a right-aligned number with thousands separators, `%(.2f)` a float with two decimals. A value of a kind the letter does not fit (`%(x)` on text) is an error that names the letter and the kind.
- Without a type letter a value prints the way `print` shows it (an integer in decimal, a float in its shortest form, `true`/`false`, text as it is), and everything that worked before (`%`, `%n`, `%%`, `%(<8)`, `format.text` with a list of texts) works exactly as before.
- The typed form is native only: the interpreter cannot yet build enum values with data, so `format.fmt` needs the native compiler. `format.text` and the helpers keep working in the interpreter.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `format-library`: typed values (`format.fmt`, `format.Value`, `to_value`) and typed layouts (precision and type letters).

## Impact

- `std/format.br`: the `Value` enum, the `Valued` interface and `to_value` methods for `int`, `float`, `text` and `bool`, the core that formats a list of values, `fmt0` to `fmt8`, and the layout parser extended with precision and type; `format.text` becomes a thin wrapper over the core.
- `src/codegen_x64.rs`: a call of `format.fmt` with N arguments is compiled as a call of `format.fmtN` (a one-line rewrite of the callee name; more than eight values is an error).
- `tests/format.rs`, `examples/report.br` (or a new example), `docs/stdlib.html`, README.
