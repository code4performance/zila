## Why

`brasa-parse --lsp` exists (syntax errors and formatting) but the VS Code extension in `editors/vscode` only has syntax highlighting, so nobody sees the language server in the editor. The roadmap's tooling item asks for "a richer VS Code extension"; connecting the editor to the server is the smallest step that makes the server useful and leaves a working base for hover, go to definition and completion later.

## What Changes

- The extension starts `brasa-parse --lsp` for `.br` files and shows its syntax errors as you type (the Problems panel and squiggles), and its formatting through "Format Document" and format on save.
- A small LSP client written in plain JavaScript inside the extension (no npm dependencies, no build step, no `node_modules`): it speaks the subset the server offers (`initialize`, `didOpen`, `didChange`, `didClose`, `publishDiagnostics`, `formatting`, `shutdown`, `exit`).
- Settings: `brasa.server.path` (the executable; default `brasa-parse`, found on `PATH`), `brasa.server.enabled` (turn the server off and keep highlighting), and `brasa.trace` for logging the protocol to an output channel. A command `Brasa: Restart Language Server`.
- When the executable cannot be started, one clear message says which path was tried and how to set it; the extension still gives highlighting.
- The extension can be packaged as a `.vsix` with one documented command, and a Node test (no VS Code needed) checks the client against the real `brasa-parse --lsp`.
- The extension version and description are updated, and the documentation and the roadmap row mention it.

## Capabilities

### New Capabilities

- `vscode-extension`: what the Brasa VS Code extension provides and how it uses the language server.

### Modified Capabilities

None.

## Impact

- `editors/vscode/` (`package.json`, new `extension.js`, `client.js`, `README`, `.vscodeignore`, `test/`), `README.md`, `docs/getting-started.html`.
- No change to the compiler or to `--lsp`. If the server needs a fix, it is a separate change.
