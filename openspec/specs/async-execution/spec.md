## Purpose

Lets a Brasa program start work that runs while the caller continues, and wait for its result with `await`, so one program can overlap many waits without blocking.

## Requirements

### Requirement: Async functions return futures
A function declared with `async` SHALL start running as its own task when it is called and SHALL return a `Future<T>` immediately, where `T` is the declared return type (or `T or error` when the function ends with `or error`). The caller SHALL continue without waiting.

#### Scenario: Calling an async function does not wait
- **WHEN** a program calls an `async` function that sleeps for 100 milliseconds and then prints a line immediately after the call
- **THEN** the line after the call prints before the sleeping function finishes

#### Scenario: Return type becomes a future
- **WHEN** a program stores the result of calling `async func f() -> int` in a variable
- **THEN** the variable has type `Future<int>`, and using it where an `int` is expected is a compile-time error

### Requirement: Await yields the future's value
`await e`, where `e` is a `Future<T>`, SHALL suspend the current task until the future has completed and SHALL then yield its value of type `T`. While the task is suspended, other tasks SHALL keep running. A future SHALL be awaitable by one `await`; awaiting it a second time SHALL stop the program with a clear error.

#### Scenario: Await returns the result
- **WHEN** a program awaits a future whose function returns 42
- **THEN** the expression evaluates to 42

#### Scenario: Await does not block other tasks
- **WHEN** three tasks each await a 100 millisecond sleep
- **THEN** all three finish after about 100 milliseconds in total, not 300

#### Scenario: Awaiting twice
- **WHEN** a program awaits the same future twice
- **THEN** the second await stops the program with an error saying the future was already awaited

### Requirement: Errors flow through await
When the awaited future's type is `T or error`, `await` SHALL yield a `T or error` value that `try`, `match` and `??` handle like any other failable call.

#### Scenario: Propagating a failure
- **WHEN** an async function returns `error("boom")` and the caller writes `try await f()` inside a function ending in `or error`
- **THEN** the caller returns the same error

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

### Requirement: Waiting for several futures
The language library SHALL provide `all(futures)`, which waits for every future in a list and yields their values in order, and `race(futures)`, which yields the value of the first future to complete.

#### Scenario: All
- **WHEN** a program awaits `all` over futures that return 1, 2 and 3 after different delays
- **THEN** it receives the list `[1, 2, 3]` in the original order

#### Scenario: Race
- **WHEN** a program awaits `race` over a 300 millisecond and a 50 millisecond future
- **THEN** it receives the value of the 50 millisecond future after about 50 milliseconds

### Requirement: Shared scheduling with tasks and channels
Async functions, `spawn`ed tasks and channels SHALL run on one scheduler and interoperate. A task SHALL switch only at an `await` (in any function it is running), `receive` on a channel, `task.yield()`, or when it finishes. The main program SHALL be a task of the same scheduler: it SHALL switch at the same points and SHALL never be starved or blocked by other tasks while it waits.

#### Scenario: Mixing channels and futures
- **WHEN** an async function sends a value on a channel and `main` receives it
- **THEN** the value arrives and neither mechanism blocks the other

#### Scenario: Main waits from a nested function
- **WHEN** `main` calls a plain function that awaits a sleeping future while a spawned task keeps printing
- **THEN** the spawned task's lines appear while the nested function waits

### Requirement: Deadlock and termination
When no task is ready, no timer is pending and no I/O operation is outstanding, the program SHALL stop with a deadlock message and a non-zero exit code. When `main` returns, the program SHALL end even if other tasks are still running.

#### Scenario: Nothing can ever wake the program
- **WHEN** `main` awaits a future that no running task will ever complete
- **THEN** the program prints a deadlock message and exits with a non-zero code

#### Scenario: Waiting on a timer is not deadlock
- **WHEN** the only remaining task is waiting on a 200 millisecond sleep
- **THEN** the program waits and continues instead of reporting deadlock

### Requirement: Memory is released
Futures, their results and the tasks behind them SHALL be released when no longer referenced. A program that starts and awaits many futures in a loop SHALL NOT grow the number of live objects without bound.

#### Scenario: Many futures
- **WHEN** a program starts and awaits 10000 async calls in sequence
- **THEN** the number of live heap objects after the loop is within a small constant of the number before it

### Requirement: Platform support
Async execution SHALL be available for the Windows native target. Compiling a bare-metal program that uses `async` or `await` SHALL fail with an error that says so.

#### Scenario: Async in a kernel
- **WHEN** a bare-metal kernel uses `async func`
- **THEN** the build fails with a message that async needs the Windows target
