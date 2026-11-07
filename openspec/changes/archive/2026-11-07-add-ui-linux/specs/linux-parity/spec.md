## MODIFIED Requirements

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
