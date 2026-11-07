## MODIFIED Requirements

### Requirement: Libraries and features that need Windows are rejected
On the Linux target, `use ui` and `use ws` SHALL be rejected at compile time with a message that names the library and says it is not available on Linux yet. Every other library (`text`, `list`, `math`, `float`, `format`, `set`, `sorted`, `bufio`, `task`, `future`, `fs`, `net`, `sys`), `async` functions, `spawn`, channels and `extern` declarations (with `#[link]`) SHALL be accepted; their behavior is described by the `linux-parity` capability.

#### Scenario: A Windows-only library
- **WHEN** a program that does `use ui` is built with `--target linux`
- **THEN** the build fails with a message saying `ui` is not available on Linux

#### Scenario: Libraries that have a Linux layer
- **WHEN** a program that uses `text`, `list`, `format`, `sorted`, `fs`, `net` and `task` is built for Linux
- **THEN** it builds and runs
