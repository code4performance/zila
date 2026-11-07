## ADDED Requirements

### Requirement: Values of any type
`format.fmt(template, v1, v2, ...)` SHALL format a template like `format.text` with up to eight arguments, each of which is an integer, a float, a boolean or text, or a value of a type that has a method `to_value()` returning a `format.Value`. It SHALL return `text or error`, with the same errors as `format.text` (missing argument, unclosed `(`, bad layout) and the same placeholders (`%`, `%n`, `%%`, `%(layout)`). More than eight values SHALL be a compile error that says so. Without a type letter a value SHALL be shown the way `print` shows it: an integer in decimal, a float in its shortest form, `true` or `false`, text as it is.

#### Scenario: Mixed values
- **WHEN** a program formats `"% has % items and costs % (paid: %)"` with `"ana"`, `3`, `2.5` and `true`
- **THEN** the result is `ana has 3 items and costs 2.5 (paid: true)`

#### Scenario: Indexes and layouts with values
- **WHEN** it formats `"%1 %0 %1"` with `7` and `"x"`, and `"[%(>5)]"` with `42`
- **THEN** the results are `x 7 x` and `[   42]`

#### Scenario: A type of the program
- **WHEN** a record `Point` has `func (p: Point) to_value() -> format.Value` returning `format.Text("(x, y)")` and a program formats `"at %"` with a `Point`
- **THEN** the result is `at (x, y)`

#### Scenario: Too many values
- **WHEN** a program passes nine values after the template
- **THEN** compilation fails saying that `format.fmt` takes at most eight values

### Requirement: Typed layouts
A layout MAY end with a precision `.N` and a type letter: `[[fill]align][0][width][.precision][type]`. The types SHALL be `d` (an integer in decimal), `x` and `X` (an integer in lower or upper case hexadecimal, negative numbers with a leading `-`), `b` (an integer in binary), `,` (an integer in decimal with a comma between groups of three digits) and `f` (a float with exactly `precision` decimals, 6 when there is none, rounding halves away from zero), `s` (any value shown as without a letter). A precision alone SHALL mean `f` and SHALL be allowed only for floats. Padding, fill, alignment and width SHALL apply to the text of the value as before. A letter that does not fit the kind of the value, or an unknown letter, SHALL give an error that names the letter (and the kind of value for a mismatch).

#### Scenario: Integers
- **WHEN** `%(x)`, `%(X)`, `%(b)` and `%(,)` format `255`, `255`, `5` and `1234567`
- **THEN** the results are `ff`, `FF`, `101` and `1,234,567`

#### Scenario: Width and fill with a type
- **WHEN** `%(08x)` formats `255`, `%(>10,)` formats `1234567` and `%(*^9d)` formats `-42`
- **THEN** the results are `000000ff`, ` 1,234,567` (one space, to make ten characters) and `***-42***`

#### Scenario: Floats
- **WHEN** `%(.2f)` formats `3.14159`, `%(f)` formats `2.5` and `%(>8.1f)` formats `-0.05`
- **THEN** the results are `3.14`, `2.500000` and `    -0.1`

#### Scenario: Letters that do not fit
- **WHEN** `%(x)` formats text, `%(f)` formats an integer, or `%(.2)` formats an integer
- **THEN** each gives an error naming the letter or the precision and the kind of value

#### Scenario: Unknown letter
- **WHEN** `%(q)` is used
- **THEN** the error says the layout `q` is bad

### Requirement: Existing formatting is unchanged
`format.text(template, args)` with a list of texts, the placeholders without a type, the layouts of `Layout` and the helpers (`pad_left`, `hex`, `thousands`, `float`, ...) SHALL behave as before.

#### Scenario: Old programs
- **WHEN** the formatting tests and examples written before this change run
- **THEN** their output is the same

### Requirement: Typed formatting is native
`format.fmt` and `format.Value` SHALL work in programs compiled to native executables. Programs run by the interpreter MAY be unable to use them, but SHALL still load `use format` and use `format.text` and the helpers as before.

#### Scenario: Interpreter
- **WHEN** a program that uses `format.text` and `format.hex` is run with `--run`
- **THEN** it prints what it printed before
