## Purpose

Makes compiled programs smaller and faster without changing what they do.

## ADDED Requirements

### Requirement: Behavior is preserved
An optimized program SHALL produce exactly the same output, exit code and run-time errors as the same program compiled with `--no-opt`, for every program the compiler accepts.

#### Scenario: Differential programs
- **WHEN** each program of the optimizer test set (arithmetic, comparisons of every operator, signed and unsigned integers, floats including NaN, loops, recursion, text, lists, maps, records, closures, `defer`, errors, generics, tasks) is compiled with and without optimization and run
- **THEN** both runs print the same output and exit with the same code

#### Scenario: Random expressions
- **WHEN** a few thousand random integer and float expressions over literals and variables are printed by a program compiled both ways
- **THEN** the outputs are identical

### Requirement: Constant folding
An expression made only of literals SHALL be computed at compile time when it is an integer, float or bool operation that cannot fail: `+`, `-`, `*`, unary `-`, `not`, comparisons, `and`, `or`, bitwise operations and shifts on integers; and `+`, `-`, `*`, `/`, unary `-` and comparisons on floats. Integer arithmetic SHALL wrap at the width of the type, as at run time, and float arithmetic SHALL follow IEEE 754. A division or remainder of integers by a constant zero SHALL NOT be folded, and SHALL still fail at run time exactly as before.

#### Scenario: Folded value
- **WHEN** a program contains `print(2 * 3 + 1)` and `print(1.5 + 2.25)`
- **THEN** the generated assembly loads `7` and the bits of `3.75` directly and contains no `imul`, `add` or `addsd` for them, and the program prints `7` and `3.75`

#### Scenario: Wrapping
- **WHEN** a program prints `9223372036854775807 + 1`
- **THEN** it prints `-9223372036854775808`, with and without optimization

#### Scenario: Division by zero stays a run-time error
- **WHEN** a program evaluates `1 / 0` for integers
- **THEN** it fails at run time with the same message as with `--no-opt`

#### Scenario: Float edge cases
- **WHEN** a program prints `0.0 / 0.0`, `1.0 / 0.0` and `-0.0 + 0.0`
- **THEN** it prints `NaN`, `inf` and `0.0`

### Requirement: Fewer temporaries and tests
A binary operation whose right operand is a literal or a local variable SHALL NOT spill the left operand to a temporary slot. A condition that is a comparison SHALL jump on the processor flags without first producing a 0 or 1. Float comparisons SHALL keep the NaN rules (every comparison with NaN is false except `!=`) in the fused form.

#### Scenario: Recursive function
- **WHEN** `fib(n: int) -> int` is compiled with optimization
- **THEN** its generated code is at least 40% shorter (in instructions) than with `--no-opt`

#### Scenario: NaN in a condition
- **WHEN** a program tests `if nan < 1.0`, `if nan >= 1.0`, `if nan != nan` and `while x < 3.0` with a NaN
- **THEN** each branch is taken exactly as it is without optimization

### Requirement: Cleanups of generated code
The optimizer SHALL remove a reload of a value that was just stored to the same place, a jump to the immediately following label, instructions that can never run after an unconditional jump up to the next label, and register moves to the same register. It SHALL NOT change code across a label or touch inline assembly blocks (`asm ... end`) or functions marked `#[naked]`.

#### Scenario: Inline assembly is untouched
- **WHEN** a function contains an `asm` block with its own jumps and labels
- **THEN** the assembly text of the block is identical with and without optimization

### Requirement: Cheaper function entry
A function that cannot own a heap value (it has no texts, lists, maps, records, closures, channels, futures or results in its slots, and no `defer`) SHALL NOT zero its local slots on entry. A function that can SHALL still zero them.

#### Scenario: Numeric function
- **WHEN** a function with only `int`, `float` and `bool` locals is compiled with optimization
- **THEN** its prologue has no slot-zeroing loop

#### Scenario: Function with a text
- **WHEN** a function with a `text` local is compiled with optimization
- **THEN** its prologue still zeroes its slots and the program leaves `live_objects()` unchanged after calling it

### Requirement: Switching it off
The command-line tool SHALL accept `--no-opt` to compile without any optimization. Without the flag, optimization SHALL be on.

#### Scenario: Flag
- **WHEN** a program is compiled with `--asm` and with `--asm --no-opt`
- **THEN** the second output is longer and contains the unfolded operations, and both are accepted by the assembler

### Requirement: Measured improvement
The benchmark programs SHALL be recorded with their instruction counts and run times, and the optimized `fib(32)` benchmark SHALL run at least 25% faster than the unoptimized one on the machine that runs the test, or the test SHALL report the measured times and fail.

#### Scenario: Benchmark
- **WHEN** `fib(32)` and a loop of 100 million additions are timed with and without optimization
- **THEN** the optimized `fib` is at least 25% faster and the optimized loop is not slower; the measured times are reported (variables still live in memory, so loops gain little until register allocation)
