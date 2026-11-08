## Purpose

Source files of the language end in `.zila`, tools that look for source files by name look for that, and messages that name a file show it.

## ADDED Requirements

### Requirement: Source files end in .zila
The source files of the language SHALL end in `.zila`. The tools that find source files by name SHALL look for `.zila` and for nothing else: the files directly in a package directory, the test files of a package (`<name>_test.zila`), the platform layers of the standard library and of the official packages (`<name>.linux.zila`), the sibling files that a program's `use` names (`<name>.zila`), and the files read to find the effects of a package. A file ending in `.br` SHALL NOT be taken for a source file by any of them.

#### Scenario: A package directory
- **WHEN** a package directory holds `util.zila` and `old.br`
- **THEN** the package is made of `util.zila` alone, and `old.br` is not read

#### Scenario: Tests
- **WHEN** a directory holds `util.zila` and `util_test.zila`
- **THEN** `util_test.zila` is left out of builds and is run by `zila-parse --test`

#### Scenario: A sibling file
- **WHEN** a program says `use helper` and `helper.zila` is next to it
- **THEN** that file is the package; a `helper.br` next to it is not

### Requirement: A file named explicitly is read whatever its name
A file given to a command by name (`zila-parse prog.txt`, `--fmt notes.src`) SHALL be read as source whatever its extension, as before; only the search for files by name depends on the extension.

#### Scenario: An odd name
- **WHEN** `zila-parse hello.txt` is run on a valid program
- **THEN** it compiles

### Requirement: Messages name the extension
Errors that name a source file SHALL use `.zila` (`main.zila:12: message`, `util.zila:3: ...`), the usage text SHALL show `<file.zila>`, and the language server SHALL read the same prefix to place a compiler error on its line.

#### Scenario: An error
- **WHEN** a program has a type error on line 7
- **THEN** the message starts with `main.zila:7:`

### Requirement: The repository uses only .zila
Every source file of the repository (examples, the standard library, the official packages, the operating-system sources, test data) SHALL end in `.zila`, and no tracked file outside `openspec/changes/` (the archive records history, and a change in progress may be about the extension itself) SHALL name `.br` as an extension. A test SHALL fail naming the file or line otherwise.

#### Scenario: A leftover
- **WHEN** a tracked file still ends in `.br`, or a page says `examples/x.br`
- **THEN** the test fails and names it
