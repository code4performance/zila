## Purpose

Gives a program the same behavior on Linux as on Windows: tasks, files, sockets, name lookup, error text, calling C and division by zero, with the same libraries on both targets.

## Requirements

### Requirement: One source, both targets
A program that uses `task`, `future` (`async`, `await`, `spawn`, channels), `fs`, `net` (TCP, UDP, name lookup), `tls`, `sys` or `ui` SHALL build with `--target linux` and with `--target windows` from the same source, and SHALL print the same output on both, apart from text that is the operating system's own wording (the reason in an error message) and values that depend on the machine (addresses, times, ports). The public API of each library SHALL be the same on both targets. Only `ws`, which is internal to `net`, SHALL be rejected, on both targets.

#### Scenario: A program with tasks and files
- **WHEN** an example that spawns tasks, sleeps, exchanges values on a channel, writes a file and reads it back is built for both targets and run
- **THEN** both print the same lines in the same order

#### Scenario: A window program
- **WHEN** a program that does `use ui` is built with `--target linux`
- **THEN** the build succeeds and the program opens its window on an X display

#### Scenario: What is still rejected
- **WHEN** a program that does `use ws` is built for either target
- **THEN** the build fails with a message saying `ws` is internal to `net`

### Requirement: Libraries are chosen by target
When the target is Linux, `use name` SHALL load `std/name.linux.zila` if that file exists and `std/name.zila` otherwise; for any other target it SHALL load `std/name.zila`. Code that does not touch the operating system (channels, futures, buffered I/O, formatting) SHALL stay in the shared file.

#### Scenario: A Linux-only layer
- **WHEN** `use fs` is compiled for Linux and for Windows
- **THEN** the Linux build uses `std/fs.linux.zila` and the Windows build uses `std/fs.zila`, and the exported functions and types are the same

### Requirement: Tasks and timers
On Linux, `spawn`, `task.yield`, `sleep`, channels (`send`, `receive`, `close`) and `async`/`await` SHALL behave as on Windows: cooperative tasks on one thread, a task runs until it waits, yields or finishes; when nothing is ready the scheduler sleeps until the next timer or I/O event; with no timer and no I/O it stops with the deadlock error; when `main` returns the program ends. Each task SHALL have its own stack that is released when the task finishes.

#### Scenario: Ordering
- **WHEN** three tasks each sleep a different time and then print
- **THEN** they print in the order of their wake-up times

#### Scenario: Deadlock
- **WHEN** a task waits for a value that nothing will send and there is no timer
- **THEN** the program stops with the same deadlock error as on Windows

#### Scenario: Many tasks
- **WHEN** ten thousand tasks are spawned and each finishes
- **THEN** all run, and memory use does not stay at ten thousand stacks

### Requirement: Files
On Linux, `fs` SHALL provide the same functions and types as on Windows: whole-file `read` and `write`, `File`, and the buffered `Reader` and `Writer` (`open`, `create`, `append`, `read_line`, `read`, `read_all`, `at_end`, `write`, `write_line`, `flush`, `close`). Errors SHALL be values (`error`) that start with the path and the reason, as `path: reason`; the reason is the text of the operating system's error (`No such file or directory`). Reading and writing SHALL NOT stop other tasks for longer than one transfer of at most 64 KiB.

#### Scenario: Round trip
- **WHEN** a program writes 1 MiB of text to a file with `fs.write` and reads it back with `fs.read`
- **THEN** the text read is equal to the text written

#### Scenario: A missing file
- **WHEN** `fs.read("no-such-file")` is awaited
- **THEN** it returns an error whose message starts with `no-such-file: `

### Requirement: Sockets
On Linux, `net` SHALL provide the same TCP and UDP functions and types as on Windows (`listen`, `accept`, `connect`, `send`, `receive`, `close`, `UdpSocket`, `bind`, `send_to`, `receive_from`, with the same address type), all non-blocking and driven by the scheduler so that a waiting task does not stop the others. A datagram larger than the buffer SHALL be reported as the same truncated-receive error on both targets, and a TCP connection that the peer closes SHALL end a read with zero bytes on both.

#### Scenario: Echo
- **WHEN** one task listens on port 0 and echoes what it receives, and another connects to it, sends `ping` and reads the answer
- **THEN** the second task prints `ping`

#### Scenario: UDP
- **WHEN** two sockets on the loopback address exchange a datagram
- **THEN** the receiver gets the bytes and the sender's address

### Requirement: Name lookup
On Linux, `net.lookup` and `net.resolve` (and their `await` forms) SHALL return the same results as on Windows for host names and literal addresses: an IPv4 literal is returned as is (like on Windows, only IPv4 addresses are used), `localhost` and names in `/etc/hosts` come from that file, and other names are asked from the servers in `/etc/resolv.conf` over UDP (with a timeout and a retry, and falling back to TCP for a truncated answer). A name that does not exist SHALL be an error that names the host. A lookup SHALL NOT stop other tasks.

#### Scenario: Local names
- **WHEN** `localhost` is resolved
- **THEN** the result is a loopback address

#### Scenario: Unknown name
- **WHEN** a name under the reserved `.invalid` top-level domain is resolved
- **THEN** the call returns an error that names the host

### Requirement: Error text
On Linux, `sys` SHALL translate an operating-system error number into its standard message (the same wording as `strerror`), and unknown numbers SHALL give `Unknown error N`. Library errors that carry a reason SHALL use it.

#### Scenario: A known number
- **WHEN** the message for error number 2 is requested
- **THEN** it is `No such file or directory`

### Requirement: Calling C
On Linux, `extern` functions declared with `#[link("name")]` SHALL be callable with the System V calling convention (integer and pointer arguments in `rdi`, `rsi`, `rdx`, `rcx`, `r8`, `r9`, the rest on the stack, a 16-byte aligned stack, result in `rax`), including callbacks (an `extern` function type) of up to six integer, pointer or float arguments. What C has buffered for its standard output SHALL be written before the program writes with `print` and when it ends, so the output keeps its order. A program that declares an `extern` SHALL be built as a dynamically linked executable that names the libraries in its `#[link]` attributes (`c` for the C library, `m` for the math library, and so on); a program without externs SHALL still be a static executable that needs no libraries.

#### Scenario: Calling the C library
- **WHEN** a program declares `extern func getpid() -> i32` with `#[link("c")]`, calls it and prints whether the result is positive
- **THEN** it prints `true`

#### Scenario: A static program stays static
- **WHEN** a program without externs is built for Linux
- **THEN** its ELF file has no interpreter and no dynamic section

### Requirement: Division by zero
An integer division or remainder by zero SHALL behave the same on both targets: the program prints `abort: <file>.zila:<line>: division by zero` to standard error and exits with status 101, like any other abort.

#### Scenario: Divide by zero
- **WHEN** a program divides an integer by a variable that is zero
- **THEN** it prints `abort: <file>.zila:<line>: division by zero` and exits with status 101 on both targets

### Requirement: The Windows build is unchanged
Apart from the division check above, building for Windows SHALL produce the same code as before this change and the bare-metal image SHALL be byte-for-byte the same (the kernel keeps its own handling of division faults).

#### Scenario: Regression
- **WHEN** the existing suites run and the boot image is rebuilt
- **THEN** all pass and the image is byte-for-byte the same

### Requirement: Compatibility is documented
`docs/native.html` SHALL list each library and feature with its status on Windows and on Linux, and the README SHALL be updated.

#### Scenario: The table
- **WHEN** the documentation test runs
- **THEN** every library in `std/` appears in the compatibility table
