## MODIFIED Requirements

### Requirement: Run compiles and executes the program
`zila <file.zila> --run` and `zila run <file.zila>` SHALL compile the program with the native compiler to a temporary executable for the machine it runs on (a PE on Windows, an ELF on Linux), run it, and delete the executable when it ends. There SHALL be no other implementation of the language behind `--run`: the output, the panics and the behavior of every library and feature SHALL be those of the executable that `--build` makes from the same file with the same optimization setting.

#### Scenario: A plain program
- **WHEN** `zila examples/hello.zila --run` or `zila run examples/hello.zila` is executed
- **THEN** the greeting is printed, the exit status is 0 and no executable is left in the temporary directory

#### Scenario: A native-only feature
- **WHEN** a program that uses `float`, generics, maps, `async` and `format.fmt` is run with `--run`
- **THEN** it behaves as its `--build` executable does

#### Scenario: Optimization
- **WHEN** `--run` is given with `--no-opt`
- **THEN** the program is built without the optimizer, as `--build --no-opt` would
