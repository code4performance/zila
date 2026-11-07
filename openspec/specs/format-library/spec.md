## Purpose

Lets programs lay out text (tables, reports, amounts) with templates and padding instead of building every line by hand.

## Requirements

### Requirement: Templates
`use format` SHALL provide `text(template, args)` where `args` is a list of texts. Each `%` SHALL be replaced by the next argument in order, `%n` (a whole number) by the argument at position `n` starting at 0, and `%%` SHALL produce `%`. Everything else SHALL be copied unchanged. (Placeholders do not use braces because a brace in a text literal starts an interpolation.) A `(` directly after `%` or `%n` starts a layout (see below) that ends at the next `)`; to write a literal `(` right after a placeholder, use an empty layout: `%()(`. A template that refers to a missing argument or has an unclosed layout or an unknown layout SHALL produce an error result, not partial text.

#### Scenario: Positional and indexed
- **WHEN** a program formats `"% is %"` with `["sky", "blue"]` and `"%1 %0 %1"` with `["a", "b"]`
- **THEN** the results are `sky is blue` and `b a b`

#### Scenario: Escaped percent
- **WHEN** a program formats `"100%% %"` with `["x"]`
- **THEN** the result is `100% x`

#### Scenario: Missing argument
- **WHEN** a program formats `"% %"` with one argument
- **THEN** the result is an error whose message names the missing argument position

#### Scenario: Unclosed layout
- **WHEN** a program formats `"a %(<5"` with one argument
- **THEN** the result is an error saying a parenthesis is not closed

### Requirement: Layout
A placeholder MAY carry a layout in parentheses: `%(W)`, `%(<W)`, `%(>W)`, `%(^W)`, with an optional fill character before the alignment (`%(*^9)`) and `%(0W)` as a shorthand for right alignment filled with zeros; it can follow an index (`%2(>8)`). `W` is the minimum width in characters; text longer than `W` SHALL never be cut. The default alignment SHALL be left. Width SHALL count UTF-8 characters, not bytes.

#### Scenario: Alignment
- **WHEN** `"[%(<5)] [%(>5)] [%(^5)]"` is formatted with `["ab", "ab", "ab"]`
- **THEN** the result is `[ab   ] [   ab] [ ab  ]`

#### Scenario: Fill and zeros
- **WHEN** `"%(*^7)"` is formatted with `["ab"]` and `"%(05)"` with `["42"]`
- **THEN** the results are `**ab***` and `00042`

#### Scenario: Wide text
- **WHEN** `"%(>3)"` is formatted with `["abcdef"]`
- **THEN** the result is `abcdef`

#### Scenario: Multi-byte text
- **WHEN** `"%(>4)"` is formatted with `["é"]`
- **THEN** the result is three spaces followed by `é`

### Requirement: Padding helpers
`pad_left(s, width)`, `pad_right(s, width)` and `center(s, width)` SHALL return `s` filled with spaces to `width` characters (when `center` cannot split the padding evenly, the extra space goes on the right), and `pad_left`, `pad_right` and `center` SHALL also be available with a fill character as a third argument.

#### Scenario: Helpers
- **WHEN** a program calls `pad_left("7", 3)`, `pad_right("7", 3)`, `center("7", 4)` and `pad_left("7", 3, "0")`
- **THEN** the results are `  7`, `7  `, ` 7  ` and `007`

### Requirement: Numbers as text
`int(n)` SHALL return the decimal text of an integer, `hex(n)` and `bin(n)` the digits in base 16 (lowercase) and 2 without a prefix (negative numbers as a minus sign followed by the digits of the magnitude), `float(x, digits)` a float with exactly `digits` decimals (rounded half away from zero), and `thousands(n)` an integer with `,` between groups of three digits.

#### Scenario: Integers
- **WHEN** a program calls `int(-42)`, `hex(255)`, `bin(5)`, `hex(-255)` and `thousands(1234567)`
- **THEN** the results are `-42`, `ff`, `101`, `-ff` and `1,234,567`

#### Scenario: Floats
- **WHEN** a program calls `float(3.14159, 2)`, `float(2.5, 0)` and `float(-0.005, 2)`
- **THEN** the results are `3.14`, `3` and `-0.01` (halves round away from zero)

#### Scenario: Largest and smallest integers
- **WHEN** a program calls `hex` and `thousands` on the largest and smallest 64-bit integers
- **THEN** the digits are correct and nothing overflows

### Requirement: A table
Formatting rows of columns SHALL work with the pieces above, as shown by an example program that prints a right-aligned table of names and amounts, and that program SHALL produce the same output natively and in the interpreter.

#### Scenario: Example
- **WHEN** `examples/report.br` is run natively and with the interpreter
- **THEN** both print the same aligned table


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
