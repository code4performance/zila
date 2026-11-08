## Purpose

The Zila Lang extension for VS Code: highlighting for `.br` files plus the language server's syntax errors and formatting, with the server's executable configurable and the extension installable as a `.vsix`.

## ADDED Requirements

### Requirement: Highlighting without a server
The extension SHALL register the `zila` language for `.br` files with syntax highlighting and bracket and comment rules, and SHALL keep doing so when the language server is disabled or cannot be started.

#### Scenario: Server not found
- **WHEN** the configured executable does not exist
- **THEN** `.br` files are still highlighted and one error message names the path that was tried and the setting that changes it

### Requirement: Syntax errors in the editor
When a `.br` document is opened or edited, the extension SHALL send its text to `zila-parse --lsp` and SHALL show the diagnostics the server publishes as errors at the reported ranges, updating them on every change and clearing them when the document is fixed or closed.

#### Scenario: A syntax error
- **WHEN** a statement with a syntax error is typed
- **THEN** the Problems panel lists one error from source `zila` on that line, and it disappears when the statement is corrected

### Requirement: Formatting
The extension SHALL provide document formatting for Zila Lang documents using the server's edits, so that "Format Document" and format on save give the same text as `zila-parse --fmt`. A document that is already formatted, or that has a syntax error, SHALL be left unchanged.

#### Scenario: Format Document
- **WHEN** a document with an unindented function body is formatted
- **THEN** the body is indented in the standard style and the document is otherwise unchanged

### Requirement: Settings
`zila.server.path` SHALL name the executable (default `zila-parse`, resolved through `PATH`), `zila.server.enabled` (default true) SHALL turn the server on and off without restarting the editor, and `zila.trace` (default off) SHALL write the messages exchanged with the server to the "Zila Lang Language Server" output channel. The command `Zila Lang: Restart Language Server` SHALL stop the running server and start a new one with the current settings.

#### Scenario: Changing the path
- **WHEN** the user sets `zila.server.path` to another executable
- **THEN** the server is restarted with it and open documents are checked again

### Requirement: Server lifetime
The extension SHALL start the server when the first Zila Lang document opens, SHALL restart it at most three times if it exits unexpectedly (then report that it stopped), and SHALL send `shutdown` and `exit` when the editor closes or the server is disabled.

#### Scenario: The server crashes
- **WHEN** the server process exits while documents are open
- **THEN** it is started again and the open documents are sent to it

### Requirement: No dependencies and one-step packaging
The extension SHALL run with no npm dependencies and no build step, and `npx @vscode/vsce package` in `editors/vscode` SHALL produce a `.vsix` that contains only the extension's files. A test that needs no editor SHALL check the client against the real `zila-parse --lsp`.

#### Scenario: Packaging
- **WHEN** the package command is run in `editors/vscode`
- **THEN** a `.vsix` is written and listing its contents shows `extension.js`, `client.js`, `package.json`, the grammar and the language configuration, and no tests
