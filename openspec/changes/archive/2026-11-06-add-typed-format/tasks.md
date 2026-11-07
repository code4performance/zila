## 1. The value core

- [x] 1.1 `std/format.br`: the `Value` enum, the `Valued` interface, `to_value` for `int`, `float`, `text` and `bool`
- [x] 1.2 Move the template engine to `values(template, []Value)`, make `text` a wrapper over it, and show the existing format tests still pass unchanged
- [x] 1.3 `fmt0` to `fmt8` and the rewrite of `format.fmt(...)` to `format.fmtN` in the code generator (including the error for more than eight values)

## 2. Typed layouts

- [x] 2.1 Parse precision and type letters (`d x X b , f s`) after the width, with one shared parsing and padding path
- [x] 2.2 Render values by type (decimal, hex, binary, thousands, fixed decimals, default form) and the error messages (kind mismatch, precision on a non-float, unknown letter)

## 3. Tests, example and documentation

- [x] 3.1 Tests for every scenario of the spec (native), a user type with `to_value`, nine values rejected, and the interpreter still running `format.text` and the helpers
- [x] 3.2 Update `examples/report.br` (or add an example) to use `format.fmt`
- [x] 3.3 `docs/stdlib.html` (the format section) and README rows
- [x] 3.4 Full test suite (async skipped) and `openspec validate add-typed-format`
