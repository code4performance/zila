## Why

`zila --lsp` reports syntax errors and formats documents, nothing more: no completion, no hover, no go-to-definition, no type or name errors until `zila` is run, and no way to debug a program except by reading generated assembly. Editing `.zila` code is blind compared to other languages. The typed tree from `add-typed-tree-emission` now gives the compiler resolved types and bindings, which is what these features need.

## What Changes

- **Semantic diagnostics**: the server runs the checker (parse, name resolution, types, effects) and reports its errors with ranges, not only parser errors.
- **Completion**: names in scope, fields and methods after `.`, package members after `pkg.`, attributes.
- **Hover**: type and doc comment of the symbol under the cursor.
- **Go to definition, find references, document and workspace symbols, rename**.
- **Signature help** for calls.
- **Debugger**: the compiler can emit line tables and local-variable info; a `zila --dap` Debug Adapter Protocol server launches the program, sets breakpoints, steps, shows the call stack and locals.
- **Editor extension**: contributes a debugger type and launch configuration, and exposes the new features without extra settings.
- Unknown requests still get a null result.

## Capabilities

### New Capabilities
- `debugger`: debug information in the produced executable and the `zila --dap` adapter.

### Modified Capabilities
- `language-server`: announces and implements completion, hover, definition, references, symbols, rename, signature help and semantic diagnostics.
- `vscode-extension`: debug contribution and the new language features.

## Impact

- `src/lsp.rs` (split into modules), reuse of the checker and `src/codegen_x64/typed.rs`; new `src/dap.rs`; debug info in `src/pe.rs` and the code generator; `editors/vscode/`.
- Depends on `add-typed-tree-emission` (typed tree, resolved bindings).
- No change to language syntax, standard library or the ABI of produced programs. Debug info is opt-in (`--debug`) and does not alter the code emitted.
