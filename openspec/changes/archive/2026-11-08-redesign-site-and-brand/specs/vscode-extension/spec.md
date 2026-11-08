## ADDED Requirements

### Requirement: Icon and description
`editors/vscode/package.json` SHALL name an `icon` (the 128 by 128 PNG of the brand, inside `editors/vscode/`), SHALL use the brand description as its `description`, and the packaged extension SHALL include the icon. The extension's README SHALL begin with the icon and the tagline.

#### Scenario: The manifest
- **WHEN** `package.json` is read
- **THEN** `icon` names an existing 128 by 128 PNG and `description` is the brand description

#### Scenario: The package
- **WHEN** the extension is packaged
- **THEN** the icon is in the `.vsix` and `.vscodeignore` does not exclude it
