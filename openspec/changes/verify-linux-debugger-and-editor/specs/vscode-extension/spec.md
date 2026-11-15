## ADDED Requirements

### Requirement: Language features
The extension SHALL expose the language server's completion, hover, signature help, definition, references, symbols, rename and semantic diagnostics in the editor, using the same server executable and settings as before, and SHALL keep highlighting working when the server is off.

#### Scenario: Completion in the editor
- **WHEN** `.` is typed after a record value in a `.zila` file
- **THEN** the editor offers the record's fields and methods

### Requirement: Debugging
The extension SHALL contribute a `zila` debugger type that runs `zila --dap`, a default launch configuration for the current file, and breakpoints in `.zila` files.

#### Scenario: Start debugging
- **WHEN** the user starts debugging a `.zila` file with the default configuration
- **THEN** the program is built with `--debug`, stops at breakpoints set in the editor, and shows locals in the Variables view
