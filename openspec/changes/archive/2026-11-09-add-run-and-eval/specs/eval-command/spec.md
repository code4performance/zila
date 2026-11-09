## Purpose
Let people run Zila code straight from the command line or a pipe, either a file or a few lines given inline, without writing a file or the boilerplate around `main`.

## ADDED Requirements

### Requirement: Run code given on the command line
`zila eval "<code>"` and `zila -e "<code>"` SHALL compile the code as a program, run it as `--run` does, and exit with the program's exit status. The code is a snippet: statements to execute, which MAY be mixed with `use` lines and top-level declarations (`func`, `async func`, `type`, `extern`, `pub` items and attributes). The statements SHALL run as the body of `func main() -> error` in `package main`, so `try` works.

#### Scenario: A one-liner
- **WHEN** `zila -e 'print(1 + 2)'` is run
- **THEN** `3` is printed and the exit status is 0

#### Scenario: Declarations and statements
- **WHEN** the snippet defines `func double(n: int) -> int` and then calls `print(double(21))`
- **THEN** `42` is printed

#### Scenario: A library
- **WHEN** the snippet starts with `use text` and calls a function of that library
- **THEN** it runs as it would in a file

#### Scenario: A failed try
- **WHEN** the snippet contains `let n = try parse("bad")` and `parse` returns an error
- **THEN** the error message is printed as a program that ends `main` with an error prints it, and the exit status is 1

### Requirement: A snippet with its own main is a complete program
When the code defines `func main`, it SHALL be compiled as written, with `package main` added only if the code has no `package` line.

#### Scenario: Complete program inline
- **WHEN** the code defines `func main()` that prints `hi`, laid out over several lines
- **THEN** `hi` is printed and nothing is wrapped around it

### Requirement: A program or a snippet from standard input
`zila run -` SHALL read a whole program from standard input, and `zila eval -` SHALL read a snippet from standard input. The tool consumes standard input before the program starts, so nothing of the program's source is left for the program to read.

#### Scenario: Here-document
- **WHEN** `zila eval -` is given `print("from a pipe")` on standard input
- **THEN** `from a pipe` is printed

#### Scenario: A whole program
- **WHEN** `zila run -` is given a complete program (with its `package` line) on standard input
- **THEN** it runs, and without the `package` line the error names `<stdin>:1:1`

### Requirement: Errors point at what the user wrote
A syntax or compile error in a snippet SHALL be reported with the line and column of the user's text, where line 1 is the first line of the snippet, and never with positions in the wrapped program. A snippet that does not compile SHALL run nothing and exit with status 1.

#### Scenario: Error on the second line
- **WHEN** the second line of a snippet has a type error
- **THEN** the message names line 2 of the snippet

### Requirement: The run subcommand
`zila run <file.zila>` SHALL behave as `zila <file.zila> --run`, including `--no-opt`, the rejection of another `--target`, the pass-through of standard input, output and error, the exit status and the removal of the temporary executable. `zila <file.zila> --run` SHALL keep working.

#### Scenario: Same as the flag
- **WHEN** `zila run examples/hello.zila` and `zila examples/hello.zila --run` are run
- **THEN** both print the same output and exit with the same status

### Requirement: Usage and documentation
The usage text and the getting-started page SHALL list `run`, `eval` and `-e`, and a test SHALL fail when an option the command line accepts is undocumented.

#### Scenario: Undocumented option
- **WHEN** `-e` is accepted by the command line but not named in `docs/getting-started.html`
- **THEN** the documentation test fails and names it
