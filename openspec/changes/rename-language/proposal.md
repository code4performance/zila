## Why

The language is going to be renamed, and the new name is not chosen yet. The current name is spread over the whole repository (about 670 occurrences in 109 files outside the archive: the crate, the `brasa-parse` command, environment variables, assembly symbols, file names, the VS Code extension, the documentation site, the OS image, the README story). Doing the rename by hand, after the name is picked, is slow and easy to get half-done. This change prepares everything now, so that the day the name is known the rename is one command, one review and one test run.

## What Changes

- **A rename tool** (`tools/rename-language.py`, standard-library Python): takes the new display name as its single required input and derives every other form (`<Name>`, `<name>`, `<NAME>`, `<name>-parse`, ...). It has a dry run that lists every file move and every replaced line, an apply mode, and a leftover check. The new name is the only thing left open.
- **An inventory** of everything that carries the name (in the design), split into what the tool does by itself and what needs a human: the README and site "Why <Name>?" story, the GOVERNANCE text, the logo emoji, the OS boot messages, the GitHub repository and the local folder.
- **A leftover test** (`tests/rename.rs`): after the rename, no tracked file outside `openspec/changes/archive/` may contain the old name in any letter case, except an explicit allow-list (a "formerly known as" note). Before the rename the test only checks the tool's own rules, so the suite stays green.
- **A rehearsal**: the tool is run on a throw-away copy with a dummy name (for example `Zorbo`), and the whole test suite, the docs check, the extension tests and the Linux tests must pass on the copy. When that passes, the real rename is a repeat with the real name.
- **A name vetting checklist** for the person choosing the name (crates.io, GitHub, the VS Code Marketplace and Open VSX publisher, domains, existing languages and commands with the same name, the file extension).
- Assumption, switchable: the source file extension **stays `.br`**. It touches 79 files and every example; if the new name wants another extension, the tool takes `--ext` and does the rest (file renames, grammar, extension, docs, tests).
- **BREAKING (at the time of the real rename):** the command (`brasa-parse`), the crate, the environment variables (`BRASA_UI_SCRIPT`, `BRASA_OLD_LINK`, `BRASA_PARSE`), the VS Code extension id, language id and setting keys (`brasa.*`), the window class and default alert title of `ui`, and the repository URL change. There are no compatibility aliases: the project is pre-1.0. A "Renamed from Brasa" note goes in the README and the docs.
- History is not rewritten: `openspec/changes/archive/` keeps the old name, because it records what was true when each change was archived.

## Capabilities

### New Capabilities

- `language-rename`: the rename tooling and the invariants that hold after a rename (nothing of the old name left, same behavior, the names users see).

### Modified Capabilities

None. The 13 main specs that mention the old name only use it in prose; their requirements do not change, so the tool rewrites their text without a spec delta.

## Impact

- New: `tools/rename-language.py`, `tests/rename.rs`, a rename section in `CONTRIBUTING.md`.
- At rename time (not now): `Cargo.toml`/`Cargo.lock`, `src/` (13 files), `std/` (22), `tests/` (23), `examples/` (14), `docs/` (9 pages), `os/` (3), `editors/vscode/`, `brasa.ebnf`, `brasa-ffi.md`, `README.md`, `GOVERNANCE.md`, `CONTRIBUTING.md`, `openspec/specs/` (13) and the active changes in `openspec/changes/` (everything except `archive/`).
- Outside the repository: the GitHub repository name and URL, the local folder name, the Marketplace publisher, and the Claude Code memory folder, which is keyed by the project folder path.
