## Purpose

Builds static x86-64 Linux executables with `--target linux`, using only the compiler, and defines what runs there and what is not available yet.

## Requirements

### Requirement: Building a Linux executable
`brasa-parse prog.br --target linux -o prog` SHALL write a static x86-64 ELF executable that needs no external tool to build and no shared library to run, and SHALL work on a machine without the Windows build tools. `--target windows` SHALL be the same as the default. `--target` with any other value SHALL be an error that lists the valid targets. With `--asm`, `--target linux` SHALL print the Linux assembly instead of building.

#### Scenario: A hello program
- **WHEN** a program that prints `hello` is built with `--target linux -o hello` and run on Linux
- **THEN** it prints `hello` and exits with status 0

#### Scenario: A bad target
- **WHEN** `--target solaris` is given
- **THEN** the tool reports that the target is unknown and names `windows` and `linux`

### Requirement: The same programs give the same output
A program that uses only the language and the libraries available on Linux SHALL print the same text on Linux as on Windows, including numbers, floats, text with multibyte characters, collections, errors and `format`. A program whose `main` returns an error SHALL print `error: <message>` and exit with status 1; a `panic` SHALL print `panic: <message>` and exit with status 101; normal completion SHALL exit with status 0. A run-time failure such as division by zero SHALL stop the program with a non-zero status.

#### Scenario: Examples agree
- **WHEN** the example programs that do not use the Windows API are built for both targets and run
- **THEN** their output is identical

#### Scenario: Exit codes
- **WHEN** a program's `main` returns `error("boom")`, and another calls `panic("stop")`
- **THEN** the first prints `error: boom` and exits with 1, and the second prints `panic: stop` and exits with 101

### Requirement: Memory
Objects SHALL be reference counted and freed as on Windows, `live_objects()` SHALL count them, and memory SHALL be given back: a loop that builds and drops large texts and lists SHALL NOT grow without bound. Running out of memory SHALL stop the program with `panic: out of memory`.

#### Scenario: Out of memory
- **WHEN** a program keeps allocating small objects that stay alive until the heap is full
- **THEN** it prints `panic: out of memory` and exits with status 101

#### Scenario: No leaks and big values
- **WHEN** a program builds and drops 160,000-byte texts and lists of 2,000 elements hundreds of times, and a list of 300,000 integers is built and summed, and `live_objects()` is compared before and after
- **THEN** the count has not grown and the program finishes in reasonable memory

### Requirement: Libraries and features that need Windows are rejected
On the Linux target, `use ui` SHALL be accepted (its behavior is described by the `ui-library` capability). `use ws` SHALL be rejected on every target, user programs included, with a message that says it is internal to `net`. Every other library (`text`, `list`, `math`, `float`, `format`, `set`, `sorted`, `bufio`, `task`, `future`, `fs`, `net`, `sys`, `tls`), `async` functions, `spawn`, channels and `extern` declarations (with `#[link]`) SHALL be accepted; their behavior is described by the `linux-parity` capability.

#### Scenario: A window program
- **WHEN** a program that does `use ui` is built with `--target linux`
- **THEN** the build succeeds

#### Scenario: A Windows-only library
- **WHEN** a program that does `use ws` is built for either target
- **THEN** the build fails with a message saying `ws` is internal to `net`

#### Scenario: Libraries that have a Linux layer
- **WHEN** a program that uses `text`, `list`, `format`, `sorted`, `fs`, `net` and `task` is built for Linux
- **THEN** it builds and runs

### Requirement: The default is unchanged
Building without `--target` or with `--target windows` SHALL produce exactly what it produced before, and the bare-metal build (`--os`) SHALL be unchanged.

#### Scenario: Regression
- **WHEN** the existing test suites for the Windows target and the operating system image run
- **THEN** all pass and the boot image is byte-for-byte the same as before

### Requirement: The option is documented
The getting-started page SHALL describe `--target`, and the native-compilation page SHALL describe the Linux target and its limits.

#### Scenario: Docs check
- **WHEN** the documentation test compares the options the tool accepts with the options the docs describe
- **THEN** `--target` is described
