## Purpose

Lets a function say "when I end, run this call", so cleanup happens on every way out without repeating it before each `return` and after each `try`.

## Requirements

### Requirement: A deferred call runs when the function ends
`defer call(...)` SHALL schedule the call to run when the enclosing function ends. The function SHALL end the same way whether it returns with `return`, runs off the end of its body, or leaves early because a `try` failed. The deferred call SHALL run before the function's own local variables are released.

#### Scenario: Running off the end
- **WHEN** a function executes `defer print("bye")` and then finishes its body
- **THEN** `bye` is printed after the rest of the body has run and before the caller continues

#### Scenario: Early return
- **WHEN** a function executes `defer print("cleanup")` and then returns early from inside an `if`
- **THEN** `cleanup` is printed before the caller receives the value

#### Scenario: Failed try
- **WHEN** a function ending in `or error` defers a call and then a `try` fails
- **THEN** the deferred call runs, and the caller still receives the same error

### Requirement: Last in, first out
Deferred calls of one function SHALL run in the reverse order of the `defer` statements that were executed.

#### Scenario: Three defers
- **WHEN** a function defers a print of `1`, then `2`, then `3`
- **THEN** the output is `3`, `2`, `1`

### Requirement: Each executed defer runs once
A `defer` statement SHALL schedule one call each time it is executed, including inside loops and conditions. A `defer` that is never reached SHALL schedule nothing. All scheduled calls run at function exit, not at the end of the loop or block.

#### Scenario: Defer in a loop
- **WHEN** a loop of three iterations executes `defer print(i)` and the function then ends
- **THEN** the three calls run at function exit in reverse order: `2`, `1`, `0`

#### Scenario: Defer not reached
- **WHEN** a `defer` sits in an `if` branch that is not taken
- **THEN** nothing is run for it

### Requirement: Variables are captured when defer runs
The variables a deferred call uses SHALL have the values they had when the `defer` statement executed. Records SHALL be shared, not copied, so a deferred method call sees the record as it is at function exit.

#### Scenario: A number changes afterwards
- **WHEN** a function sets `n = 1`, executes `defer print("n = {n}")`, and then sets `n = 2`
- **THEN** the deferred call prints `n = 1`

#### Scenario: A record changes afterwards
- **WHEN** a function executes `defer c.report()` on a record `c` and then changes a field of `c`
- **THEN** the deferred `report` sees the changed field

### Requirement: The return value is not affected
A deferred call SHALL NOT change or lose the value being returned by the function, including `T or error` results.

#### Scenario: Value survives
- **WHEN** a function returns 42 and has deferred calls that print and allocate
- **THEN** the caller receives 42

### Requirement: Failures of deferred calls are ignored
A deferred call whose result is `T or error` SHALL be accepted, and its result SHALL be discarded.

#### Scenario: Deferred call that can fail
- **WHEN** a function defers `file.close()` where `close` ends in `or error`
- **THEN** the program compiles, and a failure of that call does not change the function's own result

### Requirement: Only calls can be deferred
The operand of `defer` SHALL be a call. Any other expression SHALL be rejected at compile time with a message that says a call is needed.

#### Scenario: Not a call
- **WHEN** a program contains `defer x + 1`
- **THEN** compilation fails with an error saying `defer` needs a call

### Requirement: Where defer does and does not run
`defer` SHALL work in functions, methods, lambdas and async functions, each with its own list of deferred calls. Deferred calls SHALL NOT run on `break` or `continue`, nor when the program stops abruptly (a runtime panic or a fatal error).

#### Scenario: Break does not run it
- **WHEN** a loop body executes `defer print("x")` and then `break`
- **THEN** nothing is printed at the `break`; the call runs when the function ends

#### Scenario: Lambdas and async functions
- **WHEN** a lambda or an async function contains a `defer`
- **THEN** the deferred call runs when that lambda or async function ends

### Requirement: No memory leaks
Scheduling and running deferred calls SHALL NOT leak memory: a function with a `defer` called many times SHALL NOT increase the number of live heap objects.

#### Scenario: Many calls
- **WHEN** a function that executes a `defer` is called 5000 times
- **THEN** the number of live heap objects afterwards equals the number before

### Requirement: The interpreter agrees
The interpreter (`--run`) SHALL run deferred calls at function exit, last in first out, like the native compiler, and SHALL NOT ignore them.

#### Scenario: Interpreted program
- **WHEN** a program with two defers is run with `--run`
- **THEN** both calls run when the function ends, in reverse order
