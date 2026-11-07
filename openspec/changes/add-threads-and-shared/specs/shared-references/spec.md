## ADDED Requirements

### Requirement: shared allocates an atomically counted object
`shared expr` SHALL allocate a heap object holding the value of `expr` and yield a value of type `shared T`. Copying a `shared T` value SHALL add one reference and dropping it SHALL remove one, both with atomic operations, so the object may be referenced from several threads at once and SHALL be freed exactly once, when the last reference from any thread is dropped. `new T` SHALL keep its non-atomic counts and SHALL NOT be usable from two threads.

#### Scenario: Freed after the last thread drops it
- **WHEN** a `shared` list is passed to four threads that each drop it, and the main thread joins them and drops its own reference
- **THEN** `live_objects()` returns to its value before the `shared` was created

#### Scenario: new is not shared
- **WHEN** a program passes a `new Node` to `spawn_thread`
- **THEN** compilation fails (see the `threads` capability)

### Requirement: Shared contents are immutable by default
Through a `shared T` reference a program SHALL be able to read the value but SHALL NOT assign to its fields or elements or call its mutating operations; doing so SHALL be a compile-time error that says the content of a `shared` value is immutable and names `Mutex<T>`. Reading a field yields a copy (a number, a `shared` reference, or a copy-on-write handle), never an alias that another thread could mutate.

#### Scenario: Assigning through shared
- **WHEN** a program writes `s.count = s.count + 1` where `s` is `shared Counter`
- **THEN** compilation fails with an error saying shared content is immutable and suggesting `Mutex<Counter>`

#### Scenario: Reading is allowed
- **WHEN** several threads read `s.name` and `s.items.length` of the same `shared` record
- **THEN** each sees the values the record was created with

### Requirement: Mutex
The standard library SHALL provide `Mutex<T>`, created with `Mutex(value)` and used as `shared Mutex<T>` across threads. `m.lock()` SHALL give exclusive access to the contained `T` until the guard is released by `defer` or at the end of its block; at most one thread SHALL hold the lock at a time. `T` SHALL be transferable. Locking a mutex already held by the calling thread SHALL stop the program with a clear error (no re-entrant locking). A thread that waits for a lock SHALL sleep in the kernel without busy-waiting; tasks on that thread do not run while it waits.

#### Scenario: Counter under contention
- **WHEN** eight threads each increment a counter inside `shared Mutex<int>` 10000 times and are joined
- **THEN** the final value is 80000

#### Scenario: Re-entrant lock
- **WHEN** a thread locks a mutex it already holds
- **THEN** the program stops with an error saying the mutex is already locked by this thread

#### Scenario: Releasing with defer
- **WHEN** a function locks a mutex, defers the release, and returns early
- **THEN** another thread can lock the mutex afterwards

### Requirement: Atomic integer cell
The standard library SHALL provide `Atomic`, a cell holding one `int` with `load`, `store`, `add` (returning the previous value), `swap` and `compare_swap`, all sequentially consistent, usable as `shared Atomic` without a lock.

#### Scenario: Lock-free counter
- **WHEN** four threads each call `add(1)` on one `shared Atomic` 100000 times
- **THEN** `load()` after joining returns 400000

### Requirement: Weak references and identity on shared
A `shared T` SHALL support weak references with atomic weak counts on the same terms as `new T` (add-weak-refs). Upgrading a weak reference to a `shared` SHALL succeed only if the strong count was non-zero at the instant of the atomic upgrade, and SHALL never resurrect a freed object. Identity comparison of two `shared T` values SHALL compare object addresses and SHALL give the same answer on every thread; derived equality, ordering and hashing (add-derived-eq-ord-hash) SHALL compare the contained values.

#### Scenario: Upgrade racing the last drop
- **WHEN** the last strong reference of a `shared` object is dropped on one thread while another thread upgrades a weak reference to it
- **THEN** the upgrade yields either a live reference or "none", never a dangling one

### Requirement: Copy-on-write across threads
A copy-on-write value (add-copy-on-write) that is reachable through a `shared` reference, sent through a cross-thread channel or passed to `spawn_thread` SHALL use atomic reference-count operations, and a write to it SHALL copy when the count is greater than one, reading the count atomically. A value that cannot reach another thread SHALL keep non-atomic counts. A thread SHALL never observe another thread's write to a value it holds: writes go to a private copy.

#### Scenario: Both threads write a list received by value
- **WHEN** two threads each receive the same list and each appends a different element
- **THEN** each sees only its own element, and the original is unchanged

### Requirement: Cost is paid only where shared is used
A program that never uses `shared` or `spawn_thread` SHALL NOT execute `lock`-prefixed instructions for reference counting, and its output SHALL be unchanged.

#### Scenario: Single-thread program
- **WHEN** the assembly of a program without `shared` or `spawn_thread` is printed with `--asm`
- **THEN** reference counting of its values uses the non-atomic procedures only
