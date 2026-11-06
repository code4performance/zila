## ADDED Requirements

### Requirement: Inlining small functions
With optimization on, the compiler SHALL compile a call to a small function in place, without a call instruction, when the function's body is a single `return` of an expression of a few operations, its parameters and result are plain numbers, booleans or raw pointers, and it is an ordinary function (not a method, generic, `extern`, `async`, `naked` or recursive function, and one with no `asm`, `defer`, `try`, `await`, lambda or `match`). All other calls SHALL stay calls. Inlining SHALL NOT change what the program prints, returns or reports.

#### Scenario: A call disappears
- **WHEN** a program calls `func square(x: int) -> int return x * x end` from `main` and is compiled with `--asm`
- **THEN** the code of `main` contains no `call` of `square`, and with `--no-opt` it does

#### Scenario: Ineligible functions are still called
- **WHEN** a program calls a recursive function, a function that returns text, a method, a generic function, an `async` function and a function with an `asm` block
- **THEN** each of those calls is still a `call`

#### Scenario: The function itself remains
- **WHEN** an inlined function is also stored in a variable and called through it
- **THEN** the call works and gives the same result

### Requirement: Arguments are evaluated once, in order
An inlined call SHALL evaluate each argument exactly once, left to right, before the body, even when the parameter is unused or used several times in the body.

#### Scenario: Side effects
- **WHEN** a program calls `func second(a: int, b: int) -> int return b end` with arguments that print when evaluated, and `func twice(x: int) -> int return x + x end` with an argument that increments a counter
- **THEN** both arguments of `second` print in order, and the counter is incremented once, exactly as with `--no-opt`

### Requirement: Names inside an inlined body
The body of an inlined function SHALL see only its own parameters and the names of its own module, never the caller's local variables, and SHALL be affected by a caller's variables of the same name in no way.

#### Scenario: Shadowing
- **WHEN** a function `scale(n: int) -> int return n * factor end` uses a module-level `factor`, and the caller has a local variable also named `factor` and a variable named `n`
- **THEN** the result uses the module-level `factor` and the argument, as with `--no-opt`

#### Scenario: Another module
- **WHEN** a library function `m.f(x)` is inlined into a function of the main module that has a local named like one of the library's globals
- **THEN** the library's global is used

### Requirement: Run-time errors survive inlining
A division by zero, an out-of-range index or any other run-time failure inside an inlined body SHALL behave as it does in the unoptimized program (same message, same exit code).

#### Scenario: Division by zero in a helper
- **WHEN** an inlined `func half(a: int, b: int) -> int return a / b end` is called with `b` equal to 0
- **THEN** the program stops with the same message and exit code as with `--no-opt`

### Requirement: Inlining is switched off by `--no-opt`
With `--no-opt` no call SHALL be inlined.

#### Scenario: Flag
- **WHEN** the program of the first scenario is compiled with `--asm --no-opt`
- **THEN** `main` calls `square`

### Requirement: Measured improvement from inlining
A benchmark whose hot loop calls small helper functions SHALL run at least 25% faster with optimization than with `--no-opt` on the machine that runs the test, the existing benchmarks SHALL NOT get slower, or the test SHALL report the measured times and fail.

#### Scenario: Benchmark
- **WHEN** a loop of 100 million iterations calling three small helpers is timed with and without optimization
- **THEN** the optimized version is at least 25% faster
