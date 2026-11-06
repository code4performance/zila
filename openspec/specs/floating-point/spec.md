## Purpose

Lets a program compute with fractional numbers: averages, percentages, geometry and timings, and pass them to and from native code.

## Requirements

### Requirement: Float types
`float`, `f64` and `f32` SHALL be types usable for variables, parameters, return values, record fields, list elements, map values and `extern` signatures. `float` SHALL be identical to `f64`.

#### Scenario: Declarations
- **WHEN** a program declares `var x: float = 1.5`, `let y: f32 = 2.5` and a record with a `f64` field
- **THEN** it compiles

#### Scenario: Float as map key
- **WHEN** a program declares `Map<float, int>`
- **THEN** compilation fails with an error saying that `float` cannot be a map key

### Requirement: Float literals
A numeric literal with a fraction or exponent SHALL have type `float`, or `f32` when the expected type is `f32`. An integer literal SHALL NOT be accepted where a float is expected.

#### Scenario: Literal forms
- **WHEN** a program prints `1.5`, `2e3` and `0.25`
- **THEN** it prints `1.5`, `2000.0` and `0.25`

#### Scenario: Integer literal for a float
- **WHEN** a program writes `let x: float = 3`
- **THEN** compilation fails with an error that `int` was found where `float` was expected, suggesting `float(3)` or `3.0`

### Requirement: Arithmetic
`+`, `-`, `*`, `/` and unary `-` SHALL work on two floats of the same type. `%` SHALL be rejected for floats. Mixing `int` and `float`, or `f32` and `f64`, SHALL be a compile error. Division by zero SHALL NOT panic and SHALL follow IEEE 754.

#### Scenario: Basic operations
- **WHEN** a program prints `1.5 + 2.25`, `5.0 - 0.5`, `2.0 * 4.0`, `1.0 / 4.0` and `-(1.5)`
- **THEN** it prints `3.75`, `4.5`, `8.0`, `0.25` and `-1.5`

#### Scenario: Mixed types
- **WHEN** a program adds an `int` variable to a `float` variable
- **THEN** compilation fails with an error naming both types

#### Scenario: Division by zero
- **WHEN** a program prints `1.0 / 0.0`, `-1.0 / 0.0` and `0.0 / 0.0`
- **THEN** it prints `inf`, `-inf` and `NaN`

### Requirement: Comparison
`==`, `!=`, `<`, `<=`, `>` and `>=` SHALL compare two floats of the same type with IEEE 754 semantics: every comparison involving NaN is false, except `!=`, which is true.

#### Scenario: Ordering
- **WHEN** a program evaluates `1.5 < 2.5`, `2.5 <= 2.5` and `3.0 > 3.5`
- **THEN** the results are `true`, `true` and `false`

#### Scenario: NaN
- **WHEN** `nan` holds `0.0 / 0.0`
- **THEN** `nan == nan`, `nan < 1.0` and `nan >= 1.0` are false and `nan != nan` is true

### Requirement: Conversions
`float(x)` SHALL convert an integer to `float`. `int(x)` SHALL convert a float to `int`, truncating toward zero, and SHALL stop the program with a panic saying `float out of range` for NaN, infinities and values outside the `int` range. `f32(x)` and `f64(x)` SHALL convert between float widths, rounding when narrowing.

#### Scenario: Int to float and back
- **WHEN** a program prints `float(7) / 2.0`, `int(3.99)` and `int(-3.99)`
- **THEN** it prints `3.5`, `3` and `-3`

#### Scenario: Out of range
- **WHEN** a program evaluates `int(1.0 / 0.0)`
- **THEN** it stops with the panic `float out of range`

#### Scenario: Narrowing
- **WHEN** a program prints `f64(f32(0.1))`
- **THEN** it prints `0.10000000149011612`

### Requirement: Printing
`print`, `println` and text interpolation SHALL print a float in the shortest decimal form that converts back to the same value. A float with no fractional part SHALL print with `.0`. Large and small magnitudes SHALL use exponent form (`1e21`, `1e-7`). NaN and infinities SHALL print as `NaN`, `inf` and `-inf`. Negative zero SHALL print as `-0.0`.

#### Scenario: Shortest round trip
- **WHEN** a program prints `0.1 + 0.2` and `0.1`
- **THEN** it prints `0.30000000000000004` and `0.1`

#### Scenario: Whole and extreme values
- **WHEN** a program prints `3.0`, `1e21` and `0.0000001`
- **THEN** it prints `3.0`, `1e21` and `1e-7`

### Requirement: Math helpers
`use math` (`std/math.br`) SHALL provide `math.sqrt`, `math.abs`, `math.floor`, `math.ceil`, `math.trunc`, `math.round` (half away from zero), `math.min`, `math.max`, `math.pow` (integer exponent) and `math.fixed(x, digits)` (text with a fixed number of decimals) for `float`.

#### Scenario: Helpers
- **WHEN** a program that says `use math` prints `math.sqrt(2.25)`, `math.floor(-1.5)`, `math.ceil(1.2)`, `math.round(2.5)` and `math.fixed(3.14159, 2)`
- **THEN** it prints `1.5`, `-2.0`, `2.0`, `3.0` and `3.14`

#### Scenario: Square root of a negative
- **WHEN** a program prints `math.sqrt(-1.0)`
- **THEN** it prints `NaN`

### Requirement: Calling and ownership
Floats SHALL be passed to and returned from Brasa functions, closures and tasks like other values, and SHALL be passed to and returned from `extern` functions using the Windows x64 convention (floating arguments in XMM0 to XMM3 by position, result in XMM0). Floats SHALL NOT be reference counted.

#### Scenario: Function and closure
- **WHEN** a program passes a float to a function, a closure and a spawned task and returns a float
- **THEN** the value arrives unchanged

#### Scenario: Native call
- **WHEN** a program declares an `extern` function taking and returning `f64` bound to the C runtime's `sqrt` and calls it
- **THEN** it receives the correct result

### Requirement: Unsupported targets
The bare-metal target SHALL reject float types and literals with a message saying floats are unavailable there.

#### Scenario: OS code uses a float
- **WHEN** a program built with `--os` declares a `float`
- **THEN** compilation fails with that message

### Requirement: Interpreter agreement
`--run` SHALL give the same results and the same output text as the native compiler for `float` values: literals, arithmetic, comparison, `float()`/`int()` conversion and printing. It does not model `f32` as a separate type and does not provide `use math`.

#### Scenario: Same output
- **WHEN** `examples/floats.br` runs natively and with `--run`
- **THEN** the output is identical
