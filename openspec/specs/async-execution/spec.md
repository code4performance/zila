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
`await` SHALL be accepted only inside an `async func` or inside `func main`. Using it elsewhere SHALL be a compile-time error that names the rule.

#### Scenario: Await in a plain function
- **WHEN** a non-async function other than `main` contains an `await`
- **THEN** compilation fails with an error saying `await` needs an `async func`

### Requirement: Waiting for several futures
The language library SHALL provide `all(futures)`, which waits for every future in a list and yields their values in order, and `race(futures)`, which yields the value of the first future to complete.

#### Scenario: All
- **WHEN** a program awaits `all` over futures that return 1, 2 and 3 after different delays
- **THEN** it receives the list `[1, 2, 3]` in the original order

#### Scenario: Race
- **WHEN** a program awaits `race` over a 300 millisecond and a 50 millisecond future
- **THEN** it receives the value of the 50 millisecond future after about 50 milliseconds

### Requirement: Shared scheduling with tasks and channels
Async functions, `spawn`ed tasks and channels SHALL run on one scheduler and interoperate. A task SHALL switch only at `await`, `receive` on a channel, `task.yield()`, or when it finishes.

#### Scenario: Mixing channels and futures
- **WHEN** an async function sends a value on a channel and `main` receives it
- **THEN** the value arrives and neither mechanism blocks the other

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
