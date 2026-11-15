## ADDED Requirements

### Requirement: Installation across editors
The extension SHALL install from the `.vsix` in VS Code and in editors based on it, and the README SHALL give the install command for each, including that the editor's own `--install-extension` must be used rather than copying files.

#### Scenario: Other editor
- **WHEN** the extension is installed with that editor's `--install-extension`
- **THEN** `.zila` files are recognized there after a window reload
