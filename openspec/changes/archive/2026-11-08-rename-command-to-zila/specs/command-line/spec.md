## Purpose

The language's executable is called `zila`, and everything that tells people how to run it says so.

## ADDED Requirements

### Requirement: The executable is zila
Building the project SHALL produce an executable named `zila` (`zila.exe` on Windows) that accepts exactly the options the toolchain had before (`--run`, `--fmt`, `--fmt-check`, `--test`, `--lsp`, `--docs`, `--init`, `--add`, `--fetch`, `--tidy`, `--update`, `--explain-link`, `--target`, `-o` and the rest), and no executable named `zila-parse` SHALL be produced. The library SHALL keep the name `zila`.

#### Scenario: Building
- **WHEN** `cargo build` finishes
- **THEN** `target/debug/zila.exe` exists and `target/debug/zila-parse.exe` is not produced

#### Scenario: Same behaviour
- **WHEN** `zila hello.zila -o hello.exe` is run on a valid program
- **THEN** it builds the same executable that `zila-parse` built before

### Requirement: Messages and documentation name the command
The usage text, the hints in error messages (for example `to add one: zila --add <git-url>`), the headers of the examples, the scripts of the repository (`os/run.ps1`), the documentation site and the Markdown files SHALL write the command as `zila`. A test SHALL fail when a tracked file outside `openspec/changes/` still names the old command.

#### Scenario: The hint
- **WHEN** a program names a package that is not found
- **THEN** the error says to add it with `zila --add <git-url>`

#### Scenario: A leftover
- **WHEN** a page or a script still says the old command
- **THEN** the test fails and names the file and the line
