# abort Specification

## Purpose
Defines how a Zila Lang program fails. Overflow never fails, failures a caller can handle are values, and programmer bugs abort the process immediately.

## Requirements

### Requirement: Abort stops the process at once
An abort SHALL print one line `abort: <file>.zila:<line>: <message>` to standard error and end the process with exit status 101. It SHALL run no `defer`, no `drop` and no unwinding, SHALL NOT flush buffered writers, and SHALL NOT be catchable. An abort in any task ends the whole process. There SHALL be no `panic` in the language.

#### Scenario: Message and status
- **WHEN** a program prints `a` and then calls `abort("stop")` on line 3 of `main.zila`
- **THEN** standard output has `a`, standard error has `abort: main.zila:3: stop`, and the exit status is 101

#### Scenario: Nothing runs
- **WHEN** a function has `defer print("cleanup")` and a value with `drop`, and then aborts
- **THEN** neither the deferred call nor `drop` runs

#### Scenario: panic is gone
- **WHEN** a program calls `panic("x")`
- **THEN** compilation fails because `panic` is not a function

### Requirement: The abort function
`abort(message)` SHALL take one `text`, never return, and be the only way a program stops fatally on purpose. A function of the program named `abort` SHALL take precedence over the built-in.

#### Scenario: Used where a value is expected
- **WHEN** `abort("unreachable")` ends a branch of a function that returns `int`
- **THEN** the program compiles and the branch needs no return value

### Requirement: What aborts
The following SHALL abort with the given message: an index outside a list, slice or text (`index out of bounds`); integer division or remainder by zero (`division by zero`); reading a missing key with `m[key]` (`map key not found`); `take` on an empty list (`take from an empty list`); `int(x)` of NaN, an infinity or an out-of-range float (`float out of range`); a failed `assert` or `assert_eq` outside `--test` (`assertion failed` with the message or the two values); and running out of memory (`out of memory`, printed without a file and line because the allocator has no source line). Each, except out of memory, SHALL report the file and line of the statement that failed.

#### Scenario: Index out of bounds
- **WHEN** `xs[5]` is read on line 9 and `xs` has three items
- **THEN** the program prints `abort: <file>.zila:9: index out of bounds` to standard error and exits with 101

#### Scenario: Out of memory
- **WHEN** a program allocates until the heap is exhausted
- **THEN** it prints `abort: out of memory` and exits with 101

#### Scenario: Same on both targets
- **WHEN** the same program divides by a zero variable on Windows and on Linux
- **THEN** both print `abort: <file>.zila:<line>: division by zero` and exit with 101

### Requirement: Integer arithmetic wraps
Integer `+`, `-`, `*` and unary `-` SHALL wrap at the width of the type, in every build, optimized or not and whether computed at compile time or at run time. `MIN / -1` SHALL give `MIN` and `MIN % -1` SHALL give 0 instead of failing. Overflow SHALL never abort.

#### Scenario: Wrap
- **WHEN** a program adds 1 to the largest `int` held in a variable
- **THEN** the result is the smallest `int`, with and without `--no-opt`

#### Scenario: Narrow type
- **WHEN** an `i32` holding 2147483647 is incremented
- **THEN** the result is -2147483648

#### Scenario: Minimum divided by minus one
- **WHEN** the smallest `int` is divided by `-1` through variables
- **THEN** the result is the smallest `int` and the program does not abort

### Requirement: Checked arithmetic
The `math` library SHALL provide `checked_add(a, b)`, `checked_sub(a, b)`, `checked_mul(a, b)` and `checked_div(a, b)` for `int`, each returning `int or error`. They SHALL return the exact result when it fits and an error otherwise: overflow gives an error of kind `Overflow` and a zero divisor gives kind `InvalidInput` (kinds from `add-error-kind`). `checked_div` SHALL also report `MIN / -1` as overflow.

#### Scenario: Fits
- **WHEN** a program calls `checked_add(2, 3)`
- **THEN** the result is `ok(5)`

#### Scenario: Overflow
- **WHEN** a program calls `checked_mul` on the largest `int` and 2, or `checked_sub` on the smallest `int` and 1
- **THEN** each returns an error of kind `Overflow`

#### Scenario: Division
- **WHEN** a program calls `checked_div(1, 0)` and `checked_div` on the smallest `int` and `-1`
- **THEN** the first returns an error of kind `InvalidInput` and the second `Overflow`

### Requirement: Reading a list element without failing
A list or slice SHALL have a method `get(i)` that gives the element when `0 <= i < len` and nothing otherwise, including for negative `i`, and SHALL NOT abort. The result SHALL be `T?` (`nil` when there is no such element) when the element type can be `nil` (text, records, lists, maps and other reference types), and `T or error` (kind `InvalidInput`, message `index out of bounds`) for element types that have no `nil` (integers, floats, booleans, enums without payload), so that `xs.get(i) ?? default` works for both. `Map.get(key, default)` SHALL be unchanged.

#### Scenario: In range
- **WHEN** a program evaluates `[10, 20, 30].get(1) ?? -1`
- **THEN** the result is `20`

#### Scenario: Out of range
- **WHEN** a program evaluates `get(3)` and `get(-1)` on a list of three texts
- **THEN** both are `nil`

#### Scenario: Out of range for numbers
- **WHEN** a program evaluates `get(3) ?? -1` and `get(-1) ?? -1` on a list of three integers
- **THEN** both are `-1`, and `match` on the plain `get(3)` reaches `err(e)` with `e.kind == errors.InvalidInput`

#### Scenario: No leaks
- **WHEN** `get` is called many times on a list of texts and `live_objects()` is compared before and after
- **THEN** the count has not grown
