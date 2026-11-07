## MODIFIED Requirements

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
