## Context

`.br` appears in three kinds of places. (1) **File names**: 81 tracked files, mostly `examples/` (39), `std/` (23), `packages/` (the libraries and their Linux layers) and `os/`, including `*.linux.br` and `*_test.br` names that tools match by suffix. (2) **Code that decides by extension or builds a name**: `package.rs` (`extension() == "br"`, the `_test.br` suffix), `effects.rs`, `comptime.rs` (`<name>.br` siblings), `docs.rs` (labels), the `{}.br:{}` error prefix in `codegen_x64.rs` and `noalloc.rs`, the prefix the language server parses in `lsp.rs`, the usage text in `main.rs`, the `include_str!` paths that name library files. (3) **Text that mentions the extension**: tests (about 340 mentions, as file names in programs and fixtures), the site (about 100), Markdown, the main specs (about 60), the VS Code manifest, grammar and tests (about 35), `docs/Zila.docx`, `.vscode/settings.json`, and the archive (about 800), which is history.

The compiler never checked the extension of the file it is given; only searches by name depend on it.

## Goals / Non-Goals

**Goals:**
- One extension, `.zila`, everywhere it is decided or shown, changed in one mechanical step that the existing suite can check.
- A test that keeps `.br` from coming back.

**Non-Goals:**
- Any change to the language or the command; an alias for `.br`; rewriting the archive; migration tooling for users (there are none).

## Decisions

1. **One mechanical step, done by a one-off script that is not committed.** It does, in this order: `git mv` of every tracked `*.br` to `*.zila` (suffixes such as `.linux` and `_test` ride along because only the final extension changes); a text replacement of `\.br\b` by `.zila` in every tracked text file outside `openspec/changes/archive/` (the word boundary keeps `.brackets` and any longer word out, and `<br>` has no dot); the same replacement inside the XML of `docs/Zila.docx`; and the string comparisons that have no dot (`Some("br")` in `package.rs` and `effects.rs`, any `"br"` extension list in the extension's grammar) by hand. The script prints every distinct line it changed with a count per file, and the list is read before the commit: the only way a replacement of this kind goes wrong is a `.br` that is not the extension, and those are few enough to see.
2. **`.zila`, no alias.** Accepting both would need a rule for a directory that has `util.br` and `util.zila`, and the project has no users to protect. The search by name looks for `.zila` only; a leftover `.br` file in a package directory is ignored by the build but, like any file, still covered by the package's tree hash (that function reads every file).
3. **Explicit files are untouched.** Nothing in `cli.rs` or `main.rs` tests the extension of the argument, and no test is added that would make it a rule; the spec says so, with a scenario, so it does not change by accident.
4. **The error prefix follows the extension.** `main.br:12:` is built from the module name plus the extension in two places that are replaced by the text step; the language server's parse of the prefix and its tests are replaced the same way, and one test checks the two agree end to end (a type error shows at the right line in the editor).
5. **The editor.** The manifest's language entry lists `.zila` only, the version goes from `0.3.0` to `0.4.0` so an installed older extension is replaced, `.vscode/settings.json` (a local file, not tracked) is edited to `*.zila`, and the extension's tests and the docs page that lists the extension are covered by the text step.
6. **Specs and docs say `.zila` now; the archive does not.** The main specs are living documents and must match the code; archived changes record what was true when they were archived, as they did for the rename.
7. **A test keeps it that way.** `tests/docs.rs` (or a small test file) lists tracked files and fails on any name ending in `.br` and on any tracked text file outside the archive that matches `\.br\b`, naming the file and line. Unlike the earlier leftover test, it needs no allow-list: no note about the old extension is kept.

## Risks / Trade-offs

- A `.br` that is not the extension gets rewritten (a quotation, a Brotli mention). → The script's printed list is reviewed; `rg '\.br\b'` on the original tree is read first to see every kind of use.
- The mass rename touches almost every test fixture, so a missed pair (a file renamed but a string not, or the reverse) shows up as a failing test. → The whole suite, the docs check, the formatter check on every `.zila` file, the extension tests, the Linux tests and the boot image build are the oracle; the step is one commit on a branch.
- Windows case and path quirks in `git mv`. → Names change only in the last characters; the script checks that no target exists first.
- People with the old VS Code extension installed see `.zila` files as plain text until they update. → The version bump replaces it; no published version exists.

## Survey result (task 1.1)

772 occurrences of `.br` outside the archive (80 text files), and all of them are the extension: file names in programs and fixtures, prose such as "`.br` files", the `".br:"` prefix searched by `src/comptime/state.rs`, `strip_suffix(".br")` and `ends_with(".br")` in tests, the label `examples/&lt;name&gt;.br` in the site, and `".br"` in the extension's manifest. None is something else. Seven strings `"br"` have no dot and are changed by hand: `src/effects.rs`, `src/package.rs` (two), `tests/fmt.rs`, `tests/package.rs` (two) and `tests/parse.rs`.

## Open Questions

None that block the change. If an alias for `.br` is ever wanted, it is a separate decision with its own rule for a directory that has both.
