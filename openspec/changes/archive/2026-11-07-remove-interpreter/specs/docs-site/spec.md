## MODIFIED Requirements

### Requirement: Getting started guide
The site SHALL explain how to build the Zila Lang parser and run a `.br` file, including the `--ast`, `--tokens`, and `--run` command-line options. It SHALL describe `--run` as compiling the program to a temporary native executable for this machine, running it and passing its exit status through, and SHALL NOT describe an interpreter.

#### Scenario: Visitor wants to run a program
- **WHEN** a visitor opens the getting-started page
- **THEN** the page shows the commands to build the project and run a `.br` file, including what each of `--ast`, `--tokens`, and `--run` does, with `--run` described as compile and run
