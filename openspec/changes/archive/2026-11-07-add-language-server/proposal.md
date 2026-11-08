## Why

The roadmap's tooling item asks for a language server. Editors currently get only syntax highlighting; a server gives live error reports and formatting in any editor with an LSP client, and is the base for later features (hover, go to definition, completion).

## What Changes

- A new option `--lsp`: `zila-parse --lsp` speaks the Language Server Protocol over standard input and output.
- First feature set: syntax-error diagnostics on open and on every change (full-document sync), document formatting with the standard style of `--fmt` (one edit that replaces the document, or none when it is already formatted), and the shutdown / exit handshake.
- No new dependencies: the server carries a small JSON reader and writer.
- The command-line documentation lists `--lsp`.

## Capabilities

### New Capabilities

- `language-server`: the `--lsp` server and the features it offers.

### Modified Capabilities

None.

## Impact

`src/lsp.rs` (new), `src/cli.rs`, `src/main.rs`, `src/lib.rs`, `tests/lsp.rs` (new), `docs/getting-started.html` (the option list that `tests/docs.rs` checks). Written after the code (retroactive record).
