## MODIFIED Requirements

### Requirement: Conversions
`float(x)` SHALL convert an integer to `float`. `int(x)` SHALL convert a float to `int`, truncating toward zero, and SHALL abort with the message `float out of range` for NaN, infinities and values outside the `int` range. `f32(x)` and `f64(x)` SHALL convert between float widths, rounding when narrowing.

#### Scenario: Int to float and back
- **WHEN** a program prints `float(7) / 2.0`, `int(3.99)` and `int(-3.99)`
- **THEN** it prints `3.5`, `3` and `-3`

#### Scenario: Out of range
- **WHEN** a program evaluates `int(1.0 / 0.0)`
- **THEN** it aborts with `abort: <file>.br:<line>: float out of range` and exits with 101

#### Scenario: Narrowing
- **WHEN** a program prints `f64(f32(0.1))`
- **THEN** it prints `0.10000000149011612`
