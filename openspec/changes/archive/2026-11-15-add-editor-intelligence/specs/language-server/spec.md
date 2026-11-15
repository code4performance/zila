## MODIFIED Requirements

### Requirement: Starting the server
`zila --lsp` SHALL run a Language Server Protocol server on standard input and output, reading messages framed with `Content-Length` and answering the same way, until `exit` or the end of the input. It SHALL need no file argument. After `shutdown` followed by `exit` it SHALL exit with status 0; `exit` without `shutdown` SHALL give status 1.

#### Scenario: Handshake
- **WHEN** the client sends `initialize`
- **THEN** the answer carries the same id, announces full-text document sync, formatting, completion (triggered by `.`), hover, definition, references, document and workspace symbols, rename and signature help, and names the server

#### Scenario: Shutdown
- **WHEN** the client sends `shutdown` and then `exit`
- **THEN** the server answers `shutdown` with a null result and stops with status 0

### Requirement: Unknown requests
A request the server does not implement SHALL receive a null result, and a notification it does not implement SHALL be ignored, so that clients do not wait forever.

#### Scenario: Unknown request
- **WHEN** the client sends `textDocument/codeLens`
- **THEN** it receives an answer with a null result

## ADDED Requirements

### Requirement: Semantic diagnostics
When a document that parses is opened or changed, the server SHALL also publish the errors the compiler's checking phase would report for it (unknown names, type mismatches, wrong argument counts, effect violations), each as an error with source `zila` at the range of the offending expression. Warnings the compiler gives SHALL be published with severity 2. A document with a syntax error SHALL get only the syntax error. Diagnostics SHALL reflect unsaved edits of every open document of the package.

#### Scenario: Type error
- **WHEN** a variable declared `Int` is initialized with a text literal
- **THEN** one error is published on that expression with the checker's message, and it disappears when the text is corrected

#### Scenario: Other file edited
- **WHEN** a function used by an open file is renamed in another open file
- **THEN** the first file gets an unknown-name error without being saved

### Requirement: Completion
For `textDocument/completion` the server SHALL answer with the names visible at the cursor (locals, parameters, functions, types, imported package members), with their kind and type. After `.` it SHALL list the fields and methods of the expression's type; after a package name, that package's public members; inside an attribute, the known attributes. Items SHALL be offered even when the rest of the file does not parse.

#### Scenario: Members
- **WHEN** the cursor follows `p.` where `p` is a record
- **THEN** the record's fields and methods are listed, each with its type

#### Scenario: Broken file
- **WHEN** a statement elsewhere in the file has a syntax error
- **THEN** completion at the cursor still lists the names in scope

### Requirement: Hover and signature help
`textDocument/hover` SHALL give the type of the symbol or expression under the cursor and its doc comment; `textDocument/signatureHelp` inside a call SHALL give the callee's parameters with the current one marked. Both SHALL answer null where there is nothing to show.

#### Scenario: Hover a function
- **WHEN** the cursor is on a call to a documented function
- **THEN** the hover shows its signature and doc comment

### Requirement: Navigation
The server SHALL answer `textDocument/definition` with the declaration of the symbol under the cursor (including standard-library and other-package declarations), `textDocument/references` with every use in the package, `textDocument/documentSymbol` and `workspace/symbol` with the declarations of a file or package, and `textDocument/rename` with the edits that rename a symbol everywhere in the package, or an error when the new name clashes or the symbol is declared outside the package.

#### Scenario: Definition
- **WHEN** go to definition is used on a call of a function declared in another file
- **THEN** the location of that declaration is returned

#### Scenario: Rename clash
- **WHEN** a symbol is renamed to a name already declared in the same scope
- **THEN** the request fails with a message and no edits are made

### Requirement: Responsiveness
Requests SHALL be answered from the text of the open documents, without producing an executable, and diagnostics of a superseded edit SHALL NOT be published after those of a later one. A request about a document that changed while it was being answered SHALL get null rather than an error.

#### Scenario: Fast typing
- **WHEN** edits arrive faster than checking completes
- **THEN** only the diagnostics of the latest text are published
