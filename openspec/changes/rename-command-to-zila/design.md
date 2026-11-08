## Context

`zila-parse` appears in about 80 files outside the archive and the active changes: the `[[bin]]` name in `Cargo.toml`, the usage lines in `src/main.rs`, hints in `src/package.rs` and `src/fetch.rs`, the language server's name in `src/lsp.rs`, the tests that find the executable with `env!("CARGO_BIN_EXE_zila-parse")` or a path under `target/`, the Node tests of the extension (which take the executable from `ZILA_PARSE`-style environment and from the default setting), the header comments of 31 examples (`zila-parse examples/x.zila -o x.exe`), `os/run.ps1`, the documentation, the Markdown files and 23 lines of the main specs. The earlier renames (the name, then the file extension) were done by one mechanical step checked by the whole suite; this one has the same shape.

## Goals / Non-Goals

**Goals:**
- One executable name, `zila`, everywhere it is built, run, shown or searched.
- A test that keeps the old name out.

**Non-Goals:**
- New options or subcommands; an alias or a second executable; renaming anything else.

## Decisions

1. **Rename the binary only; keep the library name.** The package already has `[lib] name = "zila"` and gets `[[bin]] name = "zila"`. A package with a library and a binary of the same name is the Cargo default (`src/lib.rs` plus `src/main.rs`), and the tests keep `use zila::...`. Alternative: a different binary name such as `zl`; rejected, it would be a new name to learn when `zila` is free.
2. **One mechanical step, a one-off script not committed.** It replaces `zila-parse` by `zila` in every tracked text file outside `openspec/changes/` (the archive is history and the active changes are planning text), then the forms that are not the plain word are checked by hand: `CARGO_BIN_EXE_zila-parse` (replaced by the same text rule, giving `CARGO_BIN_EXE_zila`), `zila-parse.exe` in paths and in `os/run.ps1`, and any environment variable or identifier derived from the command (the survey step lists them). The script prints each changed line and counts per file, and the list is read before the commit.
3. **The collision to check first.** `zila` as the binary and `zila` as the library produce `zila.exe` and a library file of the same stem; on Windows with MSVC the debug symbol files could collide. The first task builds a throw-away copy with the new `[[bin]]` name and runs a build and one test before anything else changes, and records the result here. If Cargo reports a collision, the library is renamed in `[lib]` to `zila_lang` (a one-line change plus `use zila_lang::...` in the tests) and this design is updated.
4. **A stale executable.** `target/debug/zila-parse.exe` from earlier builds stays in `target/` (ignored); the new build does not remove it, and the leftover test does not read `target/`. The tests find the executable through Cargo, never by a fixed path, so a stale file cannot be run by mistake; the places that did use a fixed path (`os/run.ps1`, the Node tests' default) are changed to the new name.
5. **The extension.** `package.json` default `zila`, `extension.js` default and messages, its README, its Node tests, version `0.5.0`. An installed copy (done by hand on this machine in `~/.vscode/extensions`) is refreshed by copying again; the README of the extension says so. A user setting `zila.server.path` that still names the old command is left alone: it would fail with the "cannot start" message that names the command.
6. **The leftover test** is the same shape as the extension one: it lists tracked files and fails on any text file outside `openspec/changes/` that has the old command as a word, naming the line. The word is built from two parts in the test so the file does not match itself.
7. **Specs say `zila` now.** The 23 lines in the main specs are living text and change with the code; the archive does not.

## Risks / Trade-offs

- The same name for library and binary could collide in build outputs. → Decision 3 tests it first, with a fallback.
- A missed place fails a test or a script (a fixed path in a script, a hint in a message). → The whole suite, the Node tests, the boot image script and the docs check are the oracle, and the leftover test names what is left.
- `zila` is short and may match other things on a `PATH` (none known). → Noted in the first task: check `where zila` before the rename.

## Open Questions

None that block the change. If the compiler later grows subcommands (`zila build`), it is a separate change with its own compatibility story.
