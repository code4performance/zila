## MODIFIED Requirements

### Requirement: Where await is allowed
`await` SHALL be accepted in any function, method, lambda or `defer`red call, including `main`, whether or not the function is declared `async`. It SHALL suspend only the current task; the function that contains it simply does not continue until the future has completed, and every caller of that function waits with it. `await` SHALL remain an explicit expression at each wait point: no call SHALL wait without an `await` written in the source.

`await` SHALL be a compile-time error, naming the rule, inside a `drop` method and inside a function that is exported to C (`#[export]`) or used as an `extern` callback. Compiling a bare-metal program that uses `await` SHALL fail as before.

`async func` SHALL remain the declaration of a function that runs as its own task and returns `Future<T>`; it SHALL still take no `ref` parameters.

#### Scenario: Await in a plain function
- **WHEN** a non-async function other than `main` contains `await sleep(10)` and `main` calls it
- **THEN** the program compiles, and the call returns after at least 10 milliseconds with the other tasks running meanwhile

#### Scenario: Await in a function passed to map
- **WHEN** a program calls `ids.map(func(i) await fetch(i) end)` with an `async func fetch(i: int) -> int`
- **THEN** it compiles, and the result is the list of fetched values in order

#### Scenario: Await in main
- **WHEN** `main` awaits a future that a spawned task completes
- **THEN** the other task runs while `main` waits and `main` continues with the value

#### Scenario: Await in a deferred call
- **WHEN** a function defers a call that awaits `sleep(10)`
- **THEN** the deferred call runs at function exit and waits there without blocking other tasks

#### Scenario: Await in a drop method
- **WHEN** a `drop` method contains `await`
- **THEN** compilation fails with an error saying `await` cannot be used in a `drop` method

#### Scenario: Await in an extern callback
- **WHEN** a function marked `#[export("cb")]` contains `await`
- **THEN** compilation fails with an error saying `await` cannot be used in a function called from C

### Requirement: Shared scheduling with tasks and channels
Async functions, `spawn`ed tasks and channels SHALL run on one scheduler and interoperate. A task SHALL switch only at an `await` (in any function it is running), `receive` on a channel, `task.yield()`, or when it finishes. The main program SHALL be a task of the same scheduler: it SHALL switch at the same points and SHALL never be starved or blocked by other tasks while it waits.

#### Scenario: Mixing channels and futures
- **WHEN** an async function sends a value on a channel and `main` receives it
- **THEN** the value arrives and neither mechanism blocks the other

#### Scenario: Main waits from a nested function
- **WHEN** `main` calls a plain function that awaits a sleeping future while a spawned task keeps printing
- **THEN** the spawned task's lines appear while the nested function waits
