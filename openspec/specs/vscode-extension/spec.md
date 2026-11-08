## Purpose

The Zila Lang extension for VS Code: highlighting for `.zila` files plus the language server's syntax errors and formatting, with the server's executable configurable and the extension installable as a `.vsix`.

## Requirements

### Requirement: Highlighting without a server
The extension SHALL register the `zila` language for `.zila` files with syntax highlighting and bracket and comment rules, and SHALL keep doing so when the language server is disabled or cannot be started.

#### Scenario: Server not found
- **WHEN** the configured executable does not exist
- **THEN** `.zila` files are still highlighted and one error message names the path that was tried and the setting that changes it

### Requirement: Syntax errors in the editor
When a `.zila` document is opened or edited, the extension SHALL send its text to `zila --lsp` and SHALL show the diagnostics the server publishes as errors at the reported ranges, updating them on every change and clearing them when the document is fixed or closed.

#### Scenario: A syntax error
- **WHEN** a statement with a syntax error is typed
- **THEN** the Problems panel lists one error from source `zila` on that line, and it disappears when the statement is corrected

### Requirement: Formatting
The extension SHALL provide document formatting for Zila Lang documents using the server's edits, so that "Format Document" and format on save give the same text as `zila --fmt`. A document that is already formatted, or that has a syntax error, SHALL be left unchanged.

#### Scenario: Format Document
- **WHEN** a document with an unindented function body is formatted
- **THEN** the body is indented in the standard style and the document is otherwise unchanged

### Requirement: Settings
`zila.server.path` SHALL name the executable (default `zila`, resolved through `PATH`), `zila.server.enabled` (default true) SHALL turn the server on and off without restarting the editor, and `zila.trace` (default off) SHALL write the messages exchanged with the server to the "Zila Lang Language Server" output channel. The command `Zila Lang: Restart Language Server` SHALL stop the running server and start a new one with the current settings.

#### Scenario: Changing the path
- **WHEN** the user sets `zila.server.path` to another executable
- **THEN** the server is restarted with it and open documents are checked again

### Requirement: Server lifetime
The extension SHALL start the server when the first Zila Lang document opens, SHALL restart it at most three times if it exits unexpectedly (then report that it stopped), and SHALL send `shutdown` and `exit` when the editor closes or the server is disabled.

#### Scenario: The server crashes
- **WHEN** the server process exits while documents are open
- **THEN** it is started again and the open documents are sent to it

### Requirement: No dependencies and one-step packaging
The extension SHALL run with no npm dependencies and no build step, and `npx @vscode/vsce package` in `editors/vscode` SHALL produce a `.vsix` that contains only the extension's files. A test that needs no editor SHALL check the client against the real `zila --lsp`.

#### Scenario: Packaging
- **WHEN** the package command is run in `editors/vscode`
- **THEN** a `.vsix` is written and listing its contents shows `extension.js`, `client.js`, `package.json`, the grammar and the language configuration, and no tests

### Requirement: SQL inside sql blocks
The editor grammar SHALL highlight the lines of a `sql <dialect>` block as SQL (keywords, strings, comments, numbers) and the `{expr}` holes as Zila Lang expressions, and SHALL end the region at a line that is only `end`. Outside a block, an identifier `sql` SHALL not start the region.

#### Scenario: A block
- **WHEN** a file contains a `sql postgres` block with a `where id < {limit}` line
- **THEN** `select` and `where` are scoped as SQL keywords and `limit` as a Zila Lang expression

#### Scenario: A parameter named sql
- **WHEN** a line is `func run(sql: text)`
- **THEN** no SQL region starts

### Requirement: Icon and description
`editors/vscode/package.json` SHALL name an `icon` (the 128 by 128 PNG of the brand, inside `editors/vscode/`), SHALL use the brand description as its `description`, and the packaged extension SHALL include the icon. The extension's README SHALL begin with the icon and the tagline.

#### Scenario: The manifest
- **WHEN** `package.json` is read
- **THEN** `icon` names an existing 128 by 128 PNG and `description` is the brand description

#### Scenario: The package
- **WHEN** the extension is packaged
- **THEN** the icon is in the `.vsix` and `.vscodeignore` does not exclude it

### Requirement: Files of the language are opened as Zila Lang
The extension SHALL register the language `zila` for files ending in `.zila` (and for no other extension), the grammar and language configuration SHALL apply to them, and the language server SHALL start for them. The extension's version SHALL be raised so that an installed older extension, which knew another extension, is replaced.

#### Scenario: A .zila file
- **WHEN** `hello.zila` is opened
- **THEN** it is highlighted as Zila Lang and its syntax errors come from the language server

#### Scenario: The manifest
- **WHEN** `editors/vscode/package.json` is read
- **THEN** the language lists `.zila` as its only extension and the version is `0.5.0`

### Requirement: The server command is zila
The setting `zila.server.path` SHALL default to `zila`, the extension SHALL start `<command> --lsp`, and its messages and README SHALL name `zila` (for example "cannot start the language server \"zila\""). The extension's version SHALL be raised to `0.5.0`, and its README SHALL say that a copy installed by hand must be refreshed and that a setting that still names the old command is not renamed for the user.

#### Scenario: The default
- **WHEN** `package.json` is read
- **THEN** `zila.server.path` has the default `zila` and the version is `0.5.0`

#### Scenario: Not found
- **WHEN** the command cannot be started
- **THEN** the extension says so naming `zila`, keeps highlighting working, and offers to open the settings
