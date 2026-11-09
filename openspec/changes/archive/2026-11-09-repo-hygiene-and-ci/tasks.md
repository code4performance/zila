## 1. README and roadmap

- [x] 1.1 Replace the roadmap table with the real gaps (compiler structure, Linux threads, TLS server, language server, `.vsix`, library parity, targets, libraries, self-hosting), taken from the unchecked tasks under `openspec/changes/archive/`
- [x] 1.2 Remove `rename-language` and the "still open" remarks about `add-direct-pe-output`; state the test count as "more than 1,000"
- [x] 1.3 `tests/docs.rs`: `the_readme_roadmap_matches_the_open_openspec_changes`; verify it fails when the README names an archived change and when an open change is missing from it

## 2. Release

- [x] 2.1 Tag `v0.1.0` on `f8529fa`; leave `[Unreleased]` in the CHANGELOG and record this change there (Added, Removed, Fixed, Internal)

## 3. Continuous integration and clippy

- [x] 3.1 `.github/workflows/ci.yml`: build, clippy with `-D warnings`, all test targets but `async`, on Windows and Linux; `async` as a separate job
- [x] 3.2 `cargo clippy --fix`, then fix by hand or allow on the item what remains, until `cargo clippy --all-targets -- -D warnings` is clean; the whole suite (without `async`) still passes
- [x] 3.3 `CONTRIBUTING.md`: the clippy rule and what CI runs

## 4. Repository hygiene

- [x] 4.1 Remove `docs/Zila.docx`
- [x] 4.2 Document in `CONTRIBUTING.md` how to recreate `.claude/`, `.agent/` and `.opencode/` (they stay unversioned)
- [x] 4.3 Mark `threads` "Same behavior on Windows and Linux" as not yet met; check `static-linking` (it has no cache requirement, nothing to mark)

## 5. Documentation

- [x] 5.1 No page under `docs/` changes: no language, compiler, library, command-line or example behaviour changed (the README and CONTRIBUTING are not part of the site)

## Not done, left for later

Running the workflow on GitHub (first Linux run), pushing the tag, and the remaining steps of the review: the typed pipeline, hardening (fuzzing, official crypto vectors), Linux thread layer, language server features, `SECURITY.md`, an architecture document.
