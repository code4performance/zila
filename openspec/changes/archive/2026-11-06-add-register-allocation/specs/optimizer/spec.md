## ADDED Requirements

### Requirement: Variables in registers
With optimization on, the compiler SHALL keep integer, boolean and pointer local variables and parameters in processor registers where it can, and SHALL leave every other variable in memory. Keeping a variable in a register SHALL NOT change what the program prints, returns or reports.

#### Scenario: Loop variables
- **WHEN** a function with a loop updating two integer variables is compiled with `--asm`
- **THEN** the loop body reads and writes those variables in registers, with no loads or stores of them to the stack inside the loop

#### Scenario: Variables that stay in memory
- **WHEN** a variable is a float, text, list, record, is captured by a closure, or lives in a function with an `asm` block or a `defer`
- **THEN** it stays in a stack slot, exactly as with `--no-opt`

#### Scenario: Flag
- **WHEN** a program is compiled with `--no-opt`
- **THEN** no variable is kept in a register

### Requirement: Registers survive calls and tasks
A function that uses registers for variables SHALL leave the registers it uses as it found them when it returns, so that its callers, native callbacks and tasks keep their own variables.

#### Scenario: Nested and recursive calls
- **WHEN** a function with register variables calls itself, another such function, a library function and a native function, and uses its variables afterwards
- **THEN** every variable has the value the program assigned

#### Scenario: Tasks
- **WHEN** two tasks with register variables switch between each other many times
- **THEN** each task sees only its own values

## MODIFIED Requirements

### Requirement: Measured improvement
The benchmark programs SHALL be recorded with their instruction counts and run times. The optimized `fib` benchmark SHALL run at least 25% faster, and the optimized loop of 100 million iterations SHALL run at least 25% faster, than with `--no-opt` on the machine that runs the test, or the test SHALL report the measured times and fail.

#### Scenario: Benchmark
- **WHEN** `fib` and a loop of 100 million additions are timed with and without optimization
- **THEN** both optimized versions are at least 25% faster
