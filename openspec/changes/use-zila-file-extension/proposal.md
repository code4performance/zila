## Why

Source files of the language end in `.br`, the first two letters of the old name. The language is Zila Lang now, and the extension is the one place where the old name still shapes what people type and see: `hello.br`, `util_test.br`, `ui.linux.br`, `examples/*.br`, the VS Code association, the error prefix `main.br:12:`. Nothing outside this repository uses the language, so there is no one to keep compatible; the extension can simply change to `.zila`, which names the language and cannot be mistaken for another tool's files (`.br` is also the Brotli compression extension).

## What Changes

- **The extension is `.zila`.** Every source file, example, library and package file of the repository is renamed (`git mv`, so history follows): 81 files, including the platform layers (`<name>.linux.zila`) and the test files (`<name>_test.zila`).
- **The tools look for `.zila`.** A package is the `.zila` files directly in its directory; `_test.zila` files are tests; sibling packages next to a source file are `<name>.zila`; the package effects and the manifest-less builds read `.zila`. A `.br` file is no longer a source file for any of them. There is no alias and no warning: nothing is left to keep working.
- **Naming a file explicitly still works whatever its name** (`zila-parse hello.txt` compiles it): the compiler never checked the extension of the file it was given, and that does not change.
- **Messages name `.zila`**: errors are `main.zila:12: ...`, the usage text says `<file.zila>`, the language server reads the same prefix.
- **The VS Code extension** associates `.zila` (version `0.4.0`), and the grammar and language configuration follow.
- **The documentation** (the site pages, the example labels `examples/NAME.zila`, the README and the other Markdown files, the grammar file and the main specs in `openspec/specs/`) says `.zila` everywhere. `openspec/changes/archive/` is history and is not rewritten.
- A test that fails when a tracked file outside the archive still ends in `.br` or mentions it as an extension.

## Capabilities

### New Capabilities
- `source-file-extension`: the extension of source files, what looks for it, and the messages that name it.

### Modified Capabilities
- `package-manager`: a package's files and its test files use the new extension.
- `vscode-extension`: the editor association.
- `docs-site`: the example labels and the file names the pages show.

## Impact

- 81 renamed files and about 1,100 mechanical text changes outside the archive: `src/` (about 150: `package.rs`, `effects.rs`, `comptime.rs`, `docs.rs`, `lsp.rs`, `main.rs`, the `include_str!` paths and the error prefix of the code generator and `noalloc.rs`), `tests/` (about 340), `docs/` (about 100), `examples/`, `std/`, `packages/`, `os/`, `editors/vscode/`, `openspec/specs/` (about 60), the `Zila.docx` document, `.vscode/settings.json`.
- Breaking for anyone with `.br` files: they must be renamed. No such user exists outside this repository.
- Non-goals: any change to the language, a second extension kept as an alias, rewriting the archive, renaming the project or the command again.
