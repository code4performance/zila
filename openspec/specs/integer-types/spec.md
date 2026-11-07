## Purpose

Gives the integer and float types their everyday names (`int`, `long`, `float`), one documented size each, and the rules for literals, widening, conversions, lengths and error messages.

## Requirements

### Requirement: int and long aliases
`int` SHALL be a named alias of `i32` and `long` a named alias of `i64`. They SHALL NOT be distinct types: `int` and `i32`, and `long` and `i64`, SHALL be interchangeable in variables, parameters, returns, fields, generics, map keys, interface satisfaction and `extern` signatures.

#### Scenario: Interchangeable names
- **WHEN** a program assigns an `i32` value to an `int` variable and passes a `long` to an `i64` parameter
- **THEN** it compiles with no conversion

#### Scenario: Narrowing is explicit
- **WHEN** a program assigns a `long` value to an `int` variable without `int(x)`
- **THEN** compilation fails with a type mismatch error that suggests `int(x)`

### Requirement: Lossless widening
An integer value SHALL convert implicitly to a wider integer type when every value of the source type fits the target: `i8`, `i16` and `i32` (`int`) to any wider signed type, `u8`, `u16` and `u32` to any wider unsigned or signed type, and so on up to 64 bits. This SHALL apply to assignment, initialization, arguments, returns, field initializers and operands of a binary operator (the result has the wider type). It SHALL NOT apply to narrowing, to conversion between signed and unsigned of the same or smaller width, to conversion between integer and float, to type argument inference of generics (`max<T>(a: int, b: long)` is an error that asks for an explicit conversion), or to matching the signature of an interface method or a function value (those need the exact types).

#### Scenario: Widening in a call
- **WHEN** a program passes an `int` variable to a parameter of type `long`
- **THEN** it compiles and the callee receives the same numeric value

#### Scenario: Mixed arithmetic
- **WHEN** a program adds an `int` and a `long`
- **THEN** the result has type `long`

#### Scenario: No lossy conversion
- **WHEN** a program assigns a `u64`, an `i64` or a `float` value to an `int` variable, or an `i32` value to a `u64` variable
- **THEN** compilation fails with a type mismatch error

### Requirement: Arithmetic stays in the operand type
Integer arithmetic SHALL be computed in the type of its operands and wrap on overflow (see the `remove-panic-add-abort` change); an `int` operation SHALL NOT silently be done in 64 bits.

#### Scenario: Wrap at 32 bits
- **WHEN** a program multiplies two `int` values whose product does not fit 32 bits and stores the result in a `long`
- **THEN** the product has wrapped at 32 bits before it is widened, and writing `long(a) * long(b)` gives the exact product

### Requirement: Lengths and indices are int
`len` of a text, list or map, element counts, and list indices SHALL have type `int`. A list, map or text SHALL hold at most 2,147,483,647 elements (bytes for text); an operation that would exceed this SHALL stop the program as a fatal failure. Sizes and offsets of files, memory and time that can exceed that SHALL be `long`.

#### Scenario: Loop over a list
- **WHEN** a program writes `for i in 0..xs.len` and indexes `xs[i]`
- **THEN** it compiles with `i` of type `int` and no conversion

#### Scenario: File size
- **WHEN** a program reads the size of a file
- **THEN** the size has type `long`

### Requirement: float alias
`float` SHALL be a named alias of `f64`, interchangeable with it everywhere.

#### Scenario: float and f64
- **WHEN** a program passes a `float` value to an `f64` parameter
- **THEN** it compiles

### Requirement: Literal defaults
An integer literal with no other type constraint SHALL have type `int`. It SHALL adapt to an expected integer type, and a literal outside the range of its type SHALL be a compile error that suggests `long` when the value fits `i64`.

#### Scenario: Default and range
- **WHEN** a program declares `let x = 40` and `let y: long = 3000000000`
- **THEN** `x` is an `int` and both compile

#### Scenario: Literal too big for int
- **WHEN** a program declares `let z = 3000000000`
- **THEN** compilation fails with an out-of-range error that suggests `long`

### Requirement: Conversion functions
`int(x)` and `long(x)` SHALL convert any integer or float to the target type like the other sized-integer conversions, and `float(x)` SHALL convert to `float`. Float to integer conversion SHALL truncate toward zero and stop with `float out of range` for NaN and values outside the target range.

#### Scenario: Conversions
- **WHEN** a program evaluates `int(3.99)`, `long(7)` and `int(3000000000.0)`
- **THEN** the first two give 3 and 7 and the third stops with `float out of range`

### Requirement: Type names in errors
Compiler errors SHALL show `i32` as `int`, `i64` as `long` and `f64` as `float`; other integer types SHALL keep their sized names.

#### Scenario: Mismatch message
- **WHEN** a program returns `"a"` from a function declared `-> int`
- **THEN** the error says `expected int, found text`
