## ADDED Requirements

### Requirement: Files of the language are opened as Zila Lang
The extension SHALL register the language `zila` for files ending in `.zila` (and for no other extension), the grammar and language configuration SHALL apply to them, and the language server SHALL start for them. The extension's version SHALL be raised so that an installed older extension, which knew another extension, is replaced.

#### Scenario: A .zila file
- **WHEN** `hello.zila` is opened
- **THEN** it is highlighted as Zila Lang and its syntax errors come from the language server

#### Scenario: The manifest
- **WHEN** `editors/vscode/package.json` is read
- **THEN** the language lists `.zila` as its only extension and the version is `0.4.0`
