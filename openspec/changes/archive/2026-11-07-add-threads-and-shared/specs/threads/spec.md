## ADDED Requirements

### Requirement: spawn_thread starts an OS thread
`spawn_thread f(args)` SHALL evaluate `args` on the calling thread, then run `f` with them on a new OS thread, and SHALL yield a `Thread` value. `Thread.join()` SHALL wait until that thread has finished and SHALL make the thread's writes visible to the joiner. If a thread panics, the whole program SHALL stop with the panic message and status 101. When the program's `main` returns, the process SHALL end even if threads are still running. `spawn` (a task on the current thread's scheduler) SHALL behave as before. `spawn_thread` SHALL work on Windows and Linux and SHALL be rejected with a message saying so on the bare-metal kernel target.

#### Scenario: Run and join
- **WHEN** a program starts four threads that each add their index to a `shared Atomic` and joins them
- **THEN** the atomic holds 6

#### Scenario: Really parallel
- **WHEN** two threads each run a 500 ms CPU-bound loop on a machine with two or more cores and both are joined
- **THEN** the total elapsed time is clearly less than 1000 ms

#### Scenario: Panic in a thread
- **WHEN** a spawned thread calls `panic("boom")`
- **THEN** the program prints `panic: boom` and exits with status 101

### Requirement: Transferable types
The compiler SHALL compute, with no annotation, whether each type is transferable. Numbers, `bool`, `text`, enums without non-transferable payloads and `Thread` are transferable. A record, tuple, list, map, set or sorted collection is transferable if all its fields or elements (keys and values) are. A closure is transferable if all its captured values are. `shared T` and `Mutex<T>` are transferable if `T` is; `Atomic` is. `new T` and a weak reference to a `new T` are NOT transferable. A generic type is judged per instantiation, and a recursive type is transferable unless a non-transferable component is found.

#### Scenario: Plain data
- **WHEN** a record of numbers, text and a list of records of numbers is passed to `spawn_thread`
- **THEN** it compiles

#### Scenario: Non-transferable field
- **WHEN** a record with a field of type `new Node` is passed to `spawn_thread`
- **THEN** compilation fails

### Requirement: spawn_thread checks arguments and captures
Every argument of `spawn_thread` and every variable captured by a closure passed to it SHALL have a transferable type. Otherwise compilation SHALL fail with an error that names the argument or captured variable, the field path to the first non-transferable component and its type, and says that `shared` is the way to share. A mutable global `var` read or written by `f`, or by any function `f` can call, SHALL be rejected unless its type is `shared` or `Mutex`; `let` globals of transferable types are allowed.

#### Scenario: Error names the path
- **WHEN** `spawn_thread run(job)` is written where `job.owner.next` has type `new Node`
- **THEN** the error says argument `job`, path `job.owner.next`, type `new Node`, and that it is not transferable

#### Scenario: Captured variable
- **WHEN** a closure that uses a local `new` object is passed to `spawn_thread`
- **THEN** the error names the captured variable

#### Scenario: Mutable global
- **WHEN** a function started with `spawn_thread` increments a global `var counter = 0`
- **THEN** compilation fails and the error suggests `shared Atomic` or `shared Mutex<int>`

### Requirement: Thread-safe channels
A channel SHALL be safe for any number of threads sending and receiving at once. When a channel is passed to or captured by `spawn_thread`, or is held in a `shared` value, its element type SHALL be transferable, otherwise compilation fails with a path error as above. Each sender's values SHALL arrive in the order sent, and each value SHALL be received exactly once. `receive` in a task SHALL park only that task, and a `send` from another thread SHALL wake it on its own thread. `close` SHALL wake every waiting receiver. A channel used inside one thread only SHALL keep its current behavior and cost.

#### Scenario: Producers and consumer
- **WHEN** four threads each send 1000 integers on one channel and the main thread receives 4000 values
- **THEN** it receives each value exactly once and the sum is as expected

#### Scenario: Wakeup while sleeping
- **WHEN** the main thread has no ready task and waits in `receive` while another thread sends after 100 ms
- **THEN** the main thread wakes and receives the value without busy-waiting

#### Scenario: Sending a non-transferable type
- **WHEN** a `channel(new Node)` is captured by a `spawn_thread` function
- **THEN** compilation fails naming the element type

### Requirement: One cooperative scheduler per thread
Each thread SHALL have its own scheduler, run queue, timers and I/O wait set. Tasks and futures SHALL stay on the thread that created them and SHALL switch only at `await`, `receive`, `yield` or when they finish. `async` calls start tasks on the current thread. Timers and I/O completions SHALL wake tasks of the thread that owns them. Deadlock detection SHALL be per thread: a thread with nothing ready, no timer and no I/O, and that no live thread can wake, SHALL report a deadlock; a thread waiting on a channel or `join` that another live thread may still satisfy SHALL sleep instead.

#### Scenario: Tasks stay on their thread
- **WHEN** two threads each spawn tasks that print `thread.id()`
- **THEN** every task of a thread prints that thread's identifier

#### Scenario: Async on a thread
- **WHEN** a spawned thread runs an async function that sleeps, and awaits it
- **THEN** it completes on that thread without involving the main thread's scheduler

#### Scenario: Deadlock in a thread
- **WHEN** a thread awaits a future that nothing will complete and no other thread can wake it
- **THEN** the program reports a deadlock and exits with a non-zero status

### Requirement: Blocking calls on a thread
`Thread.join`, a contended `Mutex.lock` and `thread.sleep` SHALL block the calling OS thread and every task on it. A task that wants to wait without blocking its thread's other tasks SHALL use `sleep`, `await` or `receive`. A thread whose scheduler is idle SHALL sleep in the kernel until a timer, an I/O completion or a cross-thread wakeup arrives, using no CPU. `main` SHALL be allowed to `join`.

#### Scenario: Join from main
- **WHEN** `main` calls `join` on a thread that sleeps 200 ms before finishing
- **THEN** `join` returns after about 200 ms and uses almost no CPU while waiting

### Requirement: live_objects is thread safe
`live_objects()` SHALL return the number of live heap objects across all threads, maintained with atomic operations, and SHALL be exact when all other threads are joined. Threads allocating and freeing concurrently SHALL NOT corrupt the count or the allocator.

#### Scenario: No leaks after threads
- **WHEN** eight threads each allocate and drop 10000 texts and lists and are joined
- **THEN** `live_objects()` equals its value before the threads started

### Requirement: Same behavior on Windows and Linux
`spawn_thread`, `Thread`, `Mutex`, `Atomic`, `shared` and thread-safe channels SHALL behave the same on Windows and Linux, and deterministic example programs that use them SHALL print the same output on both.

#### Scenario: Examples agree
- **WHEN** the thread examples are built for both targets and run
- **THEN** their output is identical
