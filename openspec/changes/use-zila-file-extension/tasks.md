## 1. Survey

- [x] 1.1 List every distinct use of `.br` and of `"br"` outside the archive (`rg '\.br\b'` and `rg '"br"'`) and read it for any that is not the extension; write the exceptions found (if any) into design.md

## 2. The rename

- [x] 2.1 One-off script (not committed): `git mv` of the 81 `*.br` files to `*.zila`, refusing to run if a target exists; verify with `git ls-files | grep -c '\.br$'` giving 0
- [x] 2.2 The same script replaces `\.br\b` by `.zila` in every tracked text file outside `openspec/changes/archive/` and inside the XML of `docs/Zila.docx`, and prints each changed line with counts per file; verify by reading the printed list
- [x] 2.3 The comparisons without a dot, by hand: `Some("br")` in `src/package.rs` and `src/effects.rs`, and any `"br"` in `editors/vscode/` (manifest, grammar, language configuration); verify with `rg '"br"'` finding nothing
- [x] 2.4 `.vscode/settings.json` (`*.br` to `*.zila`) and the extension version `0.4.0` in `editors/vscode/package.json` and its README

## 3. Behaviour and messages

- [x] 3.1 Tests for the extension spec in `tests/package.rs`: a package directory with `util.zila` and `old.br` is made of `util.zila`; `_test.zila` is a test and `_test.br` is not; a sibling `helper.zila` is a package and `helper.br` is not; the tree hash still covers every file; duplicate-declaration errors name `.zila` files
- [x] 3.2 A test that a file named explicitly compiles whatever its name (`hello.txt`), and that an error message starts with `main.zila:7:`; the language server test for the prefix passes with it
- [x] 3.3 The test that keeps `.br` out: no tracked file name ends in `.br`, and no tracked text file outside `openspec/changes/archive/` matches `\.br\b`; verify it fails on a planted file and passes on the tree

## 4. Verification

- [ ] 4.1 `cargo build`, then the whole suite (Windows, Linux under WSL, the docs check, the extension tests, the boot image) passes; verify the formatter check on every `.zila` file with `zila-parse --fmt-check`
- [ ] 4.2 Update the pages and Markdown that describe files (the repository listing in `README.md`, `docs/getting-started.html`, the example labels checked by `cargo test --test docs`), the main specs (`openspec/specs/`, text only), and run `zila-parse --docs`; verify `cargo test --test docs` and `openspec validate --specs`
- [ ] 4.3 `git grep -n '\.br\b'` outside the archive finds nothing, and `openspec validate use-zila-file-extension` passes
