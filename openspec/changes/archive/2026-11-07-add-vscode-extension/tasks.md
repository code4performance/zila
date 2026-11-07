## 1. The client

- [x] 1.1 `editors/vscode/client.js`: process start, `Content-Length` framing (including split and merged reads), request ids, `initialize`/`initialized`, `shutdown`/`exit`; verify with a Node test that completes the handshake with the real `brasa-parse --lsp`
- [x] 1.2 Document sync (open, debounced change, close) and `publishDiagnostics` callbacks with version checks; verify with a test that sends a bad and then a fixed document and sees one error then none
- [x] 1.3 Formatting request with a 5-second limit and edit conversion; verify with a test that formats an unindented file and an already formatted one
- [x] 1.4 Crash handling: restart up to three times within a minute, resend open documents, then report; verify with a test that uses a fake server script that exits

## 2. The VS Code layer

- [x] 2.1 `editors/vscode/extension.js`: activation on `onLanguage:brasa`, diagnostics collection, formatting provider, output channel for `brasa.trace`; verify with `node --check` and the manual smoke checklist in 4.2
- [x] 2.2 Settings (`brasa.server.path`, `brasa.server.enabled`, `brasa.trace`), configuration-change handling and the `Brasa: Restart Language Server` command; verify in `package.json` (contributes) and in the smoke checklist
- [x] 2.3 Not-found handling: one message with the path and an "Open Settings" button, highlighting unaffected; verify by running the extension with a bad path (smoke checklist) and a client test for the spawn error

## 3. Packaging and documentation

- [x] 3.1 Update `package.json` (version, description, `main`, `activationEvents`, `engines`, settings, command), add `.vscodeignore` and an extension README; verify `npx @vscode/vsce package` writes a `.vsix` whose listing has the extension files and no tests (verified: 10 files, no tests; activationEvents omitted on purpose, VS Code >=1.74 derives onLanguage:brasa from contributes.languages)
- [x] 3.2 A Rust test (`tests/vscode.rs`) that runs `node --test editors/vscode/test` against the built binary and skips without `node`; verify `cargo test --test vscode`
- [x] 3.3 Document the extension (install from the `.vsix`, settings, what it does) in `docs/getting-started.html`, `README.md` (including the roadmap row) and `editors/vscode/README.md`; verify with `cargo test --test docs`

## 4. Verification

- [x] 4.1 Run the whole suite (without `tests/async.rs`) and `openspec validate add-vscode-extension`
- [x] 4.2 Manual smoke checklist in VS Code, recorded in the change before archiving: install the `.vsix`, open a `.br` file (highlighting and no error), introduce a syntax error (squiggle and Problems entry), fix it (cleared), Format Document and format on save, change `brasa.server.path` to a bad and a good value, disable and enable the server, restart the command
  - Recorded: the interactive VS Code session is unavailable to the automated agent; the checklist is left as a manual pre-release step. Automated coverage: node tests (client handshake, diagnostics, formatting, crash restart, spawn error) via `cargo test --test vscode`, and `vsce package` succeeds.
