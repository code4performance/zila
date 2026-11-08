## ADDED Requirements

### Requirement: The server command is zila
The setting `zila.server.path` SHALL default to `zila`, the extension SHALL start `<command> --lsp`, and its messages and README SHALL name `zila` (for example "cannot start the language server \"zila\""). The extension's version SHALL be raised to `0.5.0`, and its README SHALL say that a copy installed by hand must be refreshed and that a setting that still names the old command is not renamed for the user.

#### Scenario: The default
- **WHEN** `package.json` is read
- **THEN** `zila.server.path` has the default `zila` and the version is `0.5.0`

#### Scenario: Not found
- **WHEN** the command cannot be started
- **THEN** the extension says so naming `zila`, keeps highlighting working, and offers to open the settings
