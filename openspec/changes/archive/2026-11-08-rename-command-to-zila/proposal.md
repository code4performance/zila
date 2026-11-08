## Why

The command is `zila-parse`. The suffix is a leftover from the first days of the project, when the executable was a lexer and parser that printed a syntax tree. Today it is the whole toolchain: it compiles, runs, formats, tests, serves the editor and manages packages, and nobody who types `zila-parse hello.zila -o hello.exe` thinks of parsing. The language is called Zila Lang, the crate and library are already `zila`, and the source files are `.zila`; the command should be `zila`. Nothing outside this repository uses the command, so nothing needs to keep working under the old name.

## What Changes

- **The executable is `zila`** (`zila.exe` on Windows): `zila hello.zila -o hello.exe`, `zila --fmt`, `zila --test`, `zila --lsp`, `zila --add`. The command-line options are unchanged; only the name changes. `zila-parse` no longer exists.
- **Every place that runs or names the command follows**: the `[[bin]]` name in `Cargo.toml` (the library keeps its name `zila`; a package may have a library and a binary with the same name), the usage text and error hints in the source (`to add one: zila --add <git-url>`), the tests (`CARGO_BIN_EXE_zila`), the example headers, `os/run.ps1`, the documentation site, the README and the other Markdown files, the main specs, and the VS Code extension's default for `zila.server.path` and its messages.
- **The VS Code extension** starts `zila --lsp` by default (version `0.5.0`). A copy installed by hand has to be refreshed; the extension's README says how.
- **A test** that fails when a tracked file outside `openspec/changes/` still says `zila-parse`.

## Capabilities

### New Capabilities
- `command-line`: the name of the executable, that the old name does not exist, and that the usage text and hints use the new one.

### Modified Capabilities
- `vscode-extension`: the default server command and its messages.

## Impact

- About 80 files and 170 mentions outside the archive and active changes: `src/` (26: `main.rs`, `cli.rs`, `package.rs`, `fetch.rs`, `lsp.rs`), `tests/` (26, and every `env!("CARGO_BIN_EXE_zila-parse")`), `docs/` (43), `examples/` (31 header comments), `editors/vscode/` (26, with its Node tests), `openspec/specs/` (23), `os/run.ps1`, `README.md`, `CONTRIBUTING.md`, `Cargo.toml`.
- Breaking for anyone with scripts that call `zila-parse`. No such user exists.
- Non-goals: any change to the options or behaviour of the command, subcommands (`zila build`, `zila fmt`) in place of options, a shell alias for the old name, and renaming the crate, the library or the project again.
