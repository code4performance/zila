## 1. Prerequisite and structure

- [x] 1.1 Confirm `add-typed-tree-emission` provides types and resolved bindings for function bodies; list gaps. Verify: written list in this change's notes, or "none". Done: the list is in `notes.md`.
- [x] 1.2 Split `src/lsp.rs` into modules (transport, documents, diagnostics, features) without behavior change. Verify: existing LSP tests pass unchanged. Done: `src/lsp/` has `json`, `transport`, `documents`, `diagnostics`, `features` and `mod.rs` (routing); the 12 existing LSP tests and the extension tests pass unchanged, clippy clean.

## 2. Semantic diagnostics

- [ ] 2.1 Run parse + check over an overlay of open-document texts and return errors/warnings with ranges. Verify: test publishes the type-error and unknown-name scenarios from the spec.
- [ ] 2.2 Statement-level parser recovery with partial tree. Verify: test truncates every file in `examples/` at several points and the server never crashes and reports only the syntax error.
- [ ] 2.3 Debounce and drop superseded results. Verify: test sends rapid edits and sees only the last diagnostics.

## 3. Hover, completion, signature help

- [ ] 3.1 Position index (uses and declarations with ranges and targets) from the typed tree. Verify: unit test resolves every name in a sample file.
- [ ] 3.2 `textDocument/hover` with type and doc comment. Verify: LSP test on a documented function.
- [ ] 3.3 `textDocument/completion`: scope names, members after `.`, package members, attributes, in a file with an unrelated syntax error. Verify: LSP tests for the Members and Broken file scenarios.
- [ ] 3.4 `textDocument/signatureHelp`. Verify: LSP test marks the active parameter.

## 4. Navigation

- [ ] 4.1 Definition (including std and other packages) and references. Verify: LSP tests across two files and a package.
- [ ] 4.2 Document and workspace symbols. Verify: LSP test lists declarations.
- [ ] 4.3 Rename with clash and outside-package errors. Verify: LSP tests for both scenarios.
- [ ] 4.4 Update the handshake capabilities; unknown requests still return null. Verify: Handshake and Unknown request scenarios as tests.

## 5. Debug information

- [ ] 5.1 `--debug` flag writes the `.zdbg` sidecar with line table, functions, locals and type descriptors, using the code generator's layouts. Verify: executables are byte-identical with and without the flag (test), and `tools/asm-snapshot.py` shows no change.
- [ ] 5.2 Line-table test over `examples/`. Verify: every executable statement line has an address.

## 6. Debug adapter

- [ ] 6.1 Windows debugger core: launch, breakpoints, continue, single-step, memory read. Verify: test program stops at a breakpoint and continues to exit.
- [ ] 6.2 `zila --dap` protocol: initialize, launch, setBreakpoints (with moved lines), threads, stackTrace, continue/pause, next/stepIn/stepOut. Verify: scripted DAP session on a sample program visits lines in order.
- [ ] 6.3 Variables and evaluate with value rendering for every value kind; "unavailable" otherwise. Verify: scripted session checks a program with each kind of local.
- [ ] 6.4 Stop on abort and report the message. Verify: session on a program that aborts.
- [ ] 6.5 Linux core with `ptrace` (WSL ok). Verify: the 6.2 session passes on Linux; wrong-target launch fails with the spec's message.

## 7. Extension and docs

- [ ] 7.1 Extension: new capabilities, `zila` debugger contribution, launch snippet. Verify: in VS Code/Antigravity, completion appears after `.` and a breakpoint stops.
- [ ] 7.2 README: install command per editor (`code`, `antigravity-ide`), `--debug`, `--dap`; docs site page for editor features and debugging. Verify: `docs` sync check passes.
- [ ] 7.3 Run `openspec validate add-editor-intelligence` and the full test suite (leave `tests/async.rs` out per routine). Verify: both pass.
