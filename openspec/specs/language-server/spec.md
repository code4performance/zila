## Purpose

A Language Server Protocol server (`zila --lsp`) that reports syntax errors and formats documents for any editor with an LSP client.

## Requirements


### Requirement: Starting the server
`zila-parse --lsp` SHALL run a Language Server Protocol server on standard input and output, reading messages framed with `Content-Length` and answering the same way, until `exit` or the end of the input. It SHALL need no file argument. After `shutdown` followed by `exit` it SHALL exit with status 0; `exit` without `shutdown` SHALL give status 1.

#### Scenario: Handshake
- **WHEN** the client sends `initialize`
- **THEN** the answer carries the same id, announces full-text document sync and document formatting, and names the server

#### Scenario: Shutdown
- **WHEN** the client sends `shutdown` and then `exit`
- **THEN** the server answers `shutdown` with a null result and stops with status 0

### Requirement: Syntax diagnostics
When a document is opened or changed, the server SHALL publish diagnostics for it: none when it parses, and one error (severity 1, source `zila`) with the parser's message when it does not, located at the line and column of the error and extending to the end of that line. Columns SHALL count UTF-16 code units. Closing a document SHALL clear its diagnostics.

#### Scenario: A good file
- **WHEN** a file without errors is opened
- **THEN** the published diagnostics are an empty list

#### Scenario: A syntax error
- **WHEN** a file with an invalid statement on line 4 is opened
- **THEN** one error is published for line 3 (zero-based) with the parser's message

#### Scenario: Editing
- **WHEN** the client sends a change that fixes the error
- **THEN** the server publishes an empty list for that document

### Requirement: Formatting
For `textDocument/formatting` the server SHALL answer with a single edit that replaces the whole document with the text `--fmt` would produce, or with an empty list when the document is already formatted or has a syntax error.

#### Scenario: Unformatted text
- **WHEN** a document with an unindented body is formatted
- **THEN** one edit is returned whose new text is indented in the standard style and whose range covers the whole document

#### Scenario: Already formatted
- **WHEN** a formatted document is formatted again
- **THEN** the result is an empty list

### Requirement: Unknown requests
A request the server does not implement SHALL receive a null result, and a notification it does not implement SHALL be ignored, so that clients do not wait forever.

#### Scenario: Unknown request
- **WHEN** the client sends `textDocument/hover`
- **THEN** it receives an answer with a null result
