## MODIFIED Requirements

### Requirement: Where defer does and does not run
`defer` SHALL work in functions, methods, lambdas and async functions, each with its own list of deferred calls. Deferred calls SHALL NOT run on `break` or `continue`, nor when the program aborts (an `abort`, an out-of-bounds index, a division by zero and the other aborts: nothing is unwound).

#### Scenario: Break does not run it
- **WHEN** a loop body executes `defer print("x")` and then `break`
- **THEN** nothing is printed at the `break`; the call runs when the function ends

#### Scenario: Lambdas and async functions
- **WHEN** a lambda or an async function contains a `defer`
- **THEN** the deferred call runs when that lambda or async function ends

#### Scenario: Abort
- **WHEN** a function executes `defer print("x")` and then aborts
- **THEN** `x` is not printed
