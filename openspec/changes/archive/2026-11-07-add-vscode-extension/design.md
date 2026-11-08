## Context

`editors/vscode` has `package.json`, a language configuration and a TextMate grammar, and no code. `zila-parse --lsp` (spec `language-server`) offers full-document sync, syntax diagnostics and whole-document formatting over `Content-Length`-framed JSON-RPC on stdio. Node 24 and VS Code are available on the development machine; the project has no JavaScript tooling and its Rust crate has no dependencies. See proposal.md for motivation.

## Goals / Non-Goals

**Goals**
- The server is usable from VS Code with a install-and-go extension.
- The client logic is testable without VS Code.
- No npm dependencies, so nothing to install, audit or bundle.

**Non-Goals**
- Hover, go to definition, completion, rename (the server does not offer them yet).
- Publishing to the Marketplace or Open VSX, auto-downloading the compiler, bundling binaries.
- Other editors (any LSP client can already run `zila-parse --lsp`).
- Changing the server.

## Decisions

1. **A small hand-written client instead of `vscode-languageclient`.** The server uses four notifications, one request and the handshake; the client is about 200 lines (framing, request ids, document sync, diagnostics, formatting). It avoids `node_modules`, a bundler and a transitive dependency tree in a project that has none. *Alternative:* `vscode-languageclient` (full-featured, handles restarts and every capability, but adds a dependency and needs bundling for the `.vsix`); the extension can switch to it later when the server grows, since the settings and commands do not change.
2. **Split into `client.js` and `extension.js`.** `client.js` has no `vscode` import: it takes a spawn function and callbacks (`onDiagnostics(uri, list)`, `onExit(code)`, `log(line)`), so a Node test can drive it against the real binary. `extension.js` is the thin VS Code layer: documents, diagnostics collection, formatting provider, settings, commands, output channel.
3. **Sync and positions.** `textDocumentSync = 1` (full text) is sent on open and on every change (debounced by 150 ms for typing bursts). Diagnostic ranges are UTF-16 columns, which is what VS Code uses, so they map one to one. Versions are tracked so a late diagnostic for an old version is dropped.
4. **Formatting through the server.** `registerDocumentFormattingEditProvider` sends the current text (a `didChange` first if the debounce is pending), waits for `textDocument/formatting` and returns the edits as `TextEdit`s. No answer within 5 seconds returns no edits.
5. **Finding the executable.** `zila.server.path` defaults to `zila-parse`; the process is started with `spawn` (no shell). A spawn error (`ENOENT`) shows one message with the path and a button that opens the setting; highlighting keeps working. No search of the workspace for a `target/` directory: the setting is explicit.
6. **Lifetime.** The server starts lazily on the first `onLanguage:zila` activation. An unexpected exit restarts it up to three times within a minute, resending open documents, then stops with a message. `deactivate` sends `shutdown` and `exit`, then kills after 2 seconds.
7. **Packaging.** `@vscode/vsce` is used through `npx` and is not a dependency; `.vscodeignore` excludes `test/` and `*.vsix`. `engines.vscode` is raised to `^1.75.0` for the APIs used.
8. **Tests.** `editors/vscode/test/client.test.js` runs with `node --test` and uses the built `target/debug/zila-parse` (or `ZILA_BIN`), covering handshake, diagnostics on a bad and a fixed file, formatting, a crash and restart (a fake server script), and framing of split reads. The Rust test suite gets one test that runs it when `node` is available.

## Risks / Trade-offs

- [A hand-written client can drift from the protocol] → it implements only what the server announces, the Node test runs against the real server, and the switch to `vscode-languageclient` is planned for when capabilities grow.
- [Windows paths and `.exe`] → `spawn` resolves `zila-parse` through `PATH` and `PATHEXT` with `shell: false` via a small lookup; the test covers a path with spaces.
- [Formatting while the user types] → the provider flushes pending changes first, so edits apply to the text the user sees; edits are applied by VS Code atomically.
- [No automated test inside VS Code] → the VS Code layer is small and is checked by a manual smoke checklist in the tasks (open, break, fix, format, restart, disable).

## Migration Plan

Existing users of the highlighting-only extension update the extension; with no `zila-parse` on `PATH` they see the one-time message and keep highlighting. Nothing else changes.

## Open Questions

- Whether to publish to the Marketplace under the `zila` publisher name (a project decision, not needed for the code).
