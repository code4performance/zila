## Purpose

Provides awaitable timers, file access and TCP networking so a Brasa program can wait on time, disks and sockets without blocking its other tasks.

## Requirements

### Requirement: Sleeping
`sleep(ms)` SHALL return a future that completes after at least `ms` milliseconds. Sleeping SHALL NOT block other tasks. A sleep of zero or less SHALL complete after letting other ready tasks run once.

#### Scenario: Sleep duration
- **WHEN** a task awaits `sleep(100)`
- **THEN** at least 100 milliseconds pass before it continues, and other ready tasks keep running meanwhile

#### Scenario: Ordering of timers
- **WHEN** tasks sleeping 30, 10 and 20 milliseconds each print when they wake
- **THEN** the lines print in the order 10, 20, 30

### Requirement: Timeouts
`timeout(ms, future)` SHALL return a future that yields the awaited value as `T or error`: the future's value if it completes within `ms` milliseconds, otherwise an error whose message says the operation timed out. The task behind a timed-out future SHALL keep running to completion (there is no cancellation).

#### Scenario: Completes in time
- **WHEN** a 10 millisecond operation is awaited with a 500 millisecond timeout
- **THEN** the result is the operation's value

#### Scenario: Times out
- **WHEN** a 500 millisecond operation is awaited with a 50 millisecond timeout
- **THEN** the result is an error saying it timed out, after about 50 milliseconds

### Requirement: Reading and writing files
The `fs` library SHALL provide `fs.read(path)`, yielding the file's contents as text, and `fs.write(path, data)`, creating or replacing the file. Both SHALL be awaitable, SHALL NOT block other tasks while the operating system performs the transfer, and SHALL fail with an error naming the path and the reason (missing file, access denied, and so on).

#### Scenario: Round trip
- **WHEN** a program awaits `fs.write("a.txt", "hello")` and then `fs.read("a.txt")`
- **THEN** the second call yields `hello`

#### Scenario: Missing file
- **WHEN** a program reads a path that does not exist
- **THEN** the result is an error whose message names the path

#### Scenario: Large file
- **WHEN** a program reads and writes a file of several megabytes
- **THEN** the contents read back are identical to the contents written

#### Scenario: Other tasks keep running during a transfer
- **WHEN** one task reads a large file while another task prints every 10 milliseconds
- **THEN** the second task keeps printing while the read is in progress

### Requirement: TCP connections
The `net` library SHALL provide awaitable TCP operations: `net.listen(port)` returning a listener, `listener.accept()`, `net.connect(host, port)`, and on a connection `read(max)` (yielding the bytes received as text, empty when the peer closed), `write(data)` (sending all of the data) and `close()`. Every operation SHALL NOT block other tasks and SHALL fail with an error describing the cause (refused, reset, address in use, and so on).

#### Scenario: Echo over loopback
- **WHEN** a program starts a listener on a free port, a task that echoes what it reads, and a client that connects, writes `ping` and reads the reply
- **THEN** the client receives `ping`

#### Scenario: Connection refused
- **WHEN** a program connects to a local port nobody listens on
- **THEN** the result is an error saying the connection was refused

#### Scenario: Many connections
- **WHEN** 100 clients connect to one listener at the same time and each exchanges a message
- **THEN** every client receives its own reply

#### Scenario: Peer closes
- **WHEN** the peer closes the connection
- **THEN** `read` yields empty text and a further `write` fails with an error

### Requirement: Resources are released
Files, sockets and operating-system structures held by a task SHALL be released when the task finishes or the connection or listener is closed or no longer referenced, including when the task ended in an error.

#### Scenario: Handles are not leaked
- **WHEN** a program opens, uses and drops 1000 connections or files in a loop
- **THEN** the process does not run out of handles and its live object count stays bounded

### Requirement: Host name resolution
`net.connect` SHALL accept both numeric addresses and host names. Resolving a host name MAY block the scheduler while it runs; numeric addresses SHALL NOT block. The documentation SHALL state this limit.

#### Scenario: Numeric address
- **WHEN** a program connects to `127.0.0.1`
- **THEN** no blocking name lookup happens

### Requirement: Awaitable operations work from any function
Every awaitable operation of the library (`sleep`, `timeout`, `all`, `race`, `fs.read`, `fs.write`, and the `net` operations) SHALL be usable with `await` from any function, method or lambda, not only from `async func` and `main`. The operation SHALL NOT block other tasks wherever it is awaited.

#### Scenario: Reading a file in a plain function
- **WHEN** a plain function `load(path: text) -> text or error` does `return await fs.read(path)` and `main` calls it
- **THEN** it yields the file contents, and a spawned ticker task keeps running during the read

#### Scenario: Sleeping inside a lambda
- **WHEN** three lambdas that each `await sleep(100)` are called one after another from tasks started with `spawn`
- **THEN** they overlap and finish after about 100 milliseconds in total
