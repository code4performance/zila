## 1. Before changing anything

- [x] 1.1 Check that no `zila` is already on the `PATH` (`where zila`), then on a throw-away copy set `[[bin]] name = "zila"`, run `cargo build` and one integration test, and record in design.md whether library and binary of the same name collide (and the fallback if they do); verify the copy builds and the test passes
- [x] 1.2 List every use of `zila-parse` outside `openspec/changes/` and every identifier derived from it (`CARGO_BIN_EXE_...`, `zila-parse.exe`, environment variables); write any exception into design.md

## 2. The rename

- [x] 2.1 `Cargo.toml` `[[bin]] name` and the source: usage text in `src/main.rs`, hints in `src/package.rs` and `src/fetch.rs`, the server name in `src/lsp.rs`; verify `cargo build` produces `target/debug/zila.exe`
- [x] 2.2 One-off script (not committed) replacing `zila-parse` by `zila` in every tracked text file outside `openspec/changes/`, printing each changed line with counts per file; verify by reading the list
- [x] 2.3 The forms found in 1.2 by hand: fixed paths and executable names in `os/run.ps1`, the Node tests of the extension, and any test that builds a path under `target/`; verify with a search for `zila-parse` finding nothing outside `openspec/changes/`

## 3. The extension

- [x] 3.1 `editors/vscode/package.json` default `zila` and version `0.5.0`, `extension.js` default and messages, its README (refresh a hand-installed copy), its Node tests; verify `cargo test --test vscode`
- [x] 3.2 Refresh the copy installed on this machine in `~/.vscode/extensions` by copying the extension files again; verify the installed `package.json` says `0.5.0`

## 4. Tests

- [x] 4.1 Tests for the spec: `cargo build` output is `zila` and no `zila-parse` is produced (checked through `env!("CARGO_BIN_EXE_zila")` and the absence of the old file name in the manifest), the hint for an unknown package says `zila --add <git-url>`, the options listed in the spec still parse
- [x] 4.2 The leftover test: no tracked text file outside `openspec/changes/` has the old command as a word; verify it fails on a planted file and passes on the tree

## 5. Verification

- [x] 5.1 The whole suite passes (Windows, Linux under WSL, docs, extension, boot image) and `zila --fmt-check` passes on every `.zila` file; verify with `cargo test --no-fail-fast`
- [x] 5.2 Update the documentation that shows the command (the site, README, CONTRIBUTING, the example headers) and run `zila --docs`; verify `cargo test --test docs` and `openspec validate --specs`, and that `zila hello.zila -o hello.exe` builds and runs a program
