## ADDED Requirements

### Requirement: Run compiles and executes the program
`brasa-parse <file.br> --run` SHALL compile the program with the native compiler to a temporary executable for the machine it runs on (a PE on Windows, an ELF on Linux), run it, and delete the executable when it ends. There SHALL be no other implementation of the language behind `--run`: the output, the panics and the behavior of every library and feature SHALL be those of the executable that `--build` makes from the same file with the same optimization setting.

#### Scenario: A plain program
- **WHEN** `brasa-parse examples/hello.br --run` is executed
- **THEN** the greeting is printed, the exit status is 0 and no executable is left in the temporary directory

#### Scenario: A native-only feature
- **WHEN** a program that uses `float`, generics, maps, `async` and `format.fmt` is run with `--run`
- **THEN** it behaves as its `--build` executable does

#### Scenario: Optimization
- **WHEN** `--run` is given with `--no-opt`
- **THEN** the program is built without the optimizer, as `--build --no-opt` would

### Requirement: Output and status pass through
The program SHALL inherit the standard input, output and error of `brasa-parse`, with nothing added or buffered by the tool, and `brasa-parse` SHALL exit with the program's exit status (101 after a panic, as the executable would); on Linux a program ended by a signal SHALL give `128` plus the signal number.

#### Scenario: Exit status
- **WHEN** a program that panics is run with `--run`
- **THEN** its `panic:` line is printed as the executable prints it and `brasa-parse` exits with 101

#### Scenario: Standard input
- **WHEN** a program that reads a line from standard input is run with `--run` with text piped in
- **THEN** the program reads that text

### Requirement: Build errors run nothing
When the program does not parse or compile, `--run` SHALL report the error as `--build` does, SHALL NOT run anything and SHALL exit with status 1. A temporary file written before the failure SHALL be removed.

#### Scenario: Type error
- **WHEN** a program with a type error is run with `--run`
- **THEN** a `compilation error:` message is printed, nothing runs and the status is 1

### Requirement: Host target only
`--run` SHALL build for the host. An explicit `--target` naming another target SHALL be rejected, before anything is compiled, with a message that says `--run` runs only programs for this machine and suggests `--build`.

#### Scenario: Other target
- **WHEN** `--run --target linux` is given on Windows
- **THEN** the tool prints that `--run` can only run programs built for this machine and exits with status 1 without compiling

### Requirement: Reference float layout stays public
`brasa::float_text` and `brasa::float_text_f32` SHALL remain available with unchanged results (the shortest text that reads back as the same number, plain notation between 1e-4 and 1e16, exponent form outside, `NaN`, `inf`, `-inf`, `0.0`), as the Rust-side reference the tests compare native output against.

#### Scenario: Same results
- **WHEN** `float_text(0.1 + 0.2)` is called
- **THEN** it returns `0.30000000000000004`
