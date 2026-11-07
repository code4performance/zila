## 1. Manifest, lock, semver, hashing (`src/package.rs`, new, registered in `src/lib.rs`)

- [ ] 1.1 TOML-subset parser, `Manifest` and `Lock` types with writers; errors carry line numbers. Verify: unit tests for valid files, round trip, and each rejected construct (array, inline table, unknown key, bad version); `edition` accepted and ignored.
- [ ] 1.2 Semver parsing and ordering, tag filtering by major, module path derivation from URLs (`https://`, `ssh://`, `git@host:path`, `file://`). Verify: unit tests including `/v2` suffix matching and tags of the wrong major being ignored.
- [ ] 1.3 Minimal version selection over an in-memory requirement graph. Verify: unit tests for the two-requirements scenario, a diamond, majors as separate packages, and determinism (same graph, same output in any input order).
- [ ] 1.4 SHA-256 and the tree hash. Verify: standard test vectors for the digest, and a tree-hash test that is independent of directory listing order and changes with a one-byte edit or a rename.

## 2. Fetching and cache

- [ ] 2.1 Git runner: `ls-remote --tags`, clone without checkout, detached checkout, `.git` removal, `GIT_TERMINAL_PROMPT=0`, the missing-git message and the moved-tag message. Verify: `tests/package.rs` builds local `file://` repositories with tags (skips with a message when `git` is not usable) and checks fetch, annotated tags, a moved tag, and the missing-git message by running with an empty `PATH`.
- [ ] 2.2 Cache under `BRASA_HOME` / `~/.brasa`, atomic write by rename, lookup by module path, version and commit. Verify: unit tests with a temporary `BRASA_HOME`; an interrupted write leaves no entry.
- [ ] 2.3 Resolver entry point `package::resolve_project(source_path)`: finds the manifest upward, reads the lock, checks coverage and hashes, returns the packages with their parsed files and import-name maps; never calls git. Verify: tests for stale lock, tampered cache, missing cache entry, with the exact messages; a test that the build path has no reference to the git runner (the runner lives in a module only `--add`, `--fetch` and `--update` import).

## 3. Command line (`src/cli.rs`, `src/main.rs`)

- [ ] 3.1 Add `--init`, `--add`, `--as`, `--fetch`, `--tidy`, `--update`, `--offline` to `FLAGS`, `Mode` and `parse`; value options take their argument like `--target`. Verify: the existing `every_flag_is_accepted` test extended for the value options, plus parse tests.
- [ ] 3.2 Dispatch in `main.rs` before the "source path required" step; implement the commands with `package.rs`. Verify: `tests/package.rs` runs the binary through `--init`, `--add`, `--fetch`, `--tidy`, `--update`, `--offline` against local repositories and checks manifest, lock and cache contents and exit codes, including the reserved-name refusal.

## 4. Compiler integration (`src/codegen_x64.rs`)

- [ ] 4.1 Make `Compiler::load` call the resolver when `std_source_for` finds nothing, and turn unknown names into the `unknown package` error. Verify: `tests/codegen.rs` cases for a program with a dependency, and for an unknown name (message and hint).
- [ ] 4.2 Re-key modules by identity (`<module path>@<major>` or directory path) with mangled symbols, keeping import names as the user-visible qualifier; update `self.modules`, `self.imports`, `cur_mod` and qualified-name construction. Verify: the whole existing suite passes with identical assembly for programs that use only `std/` (compare `--asm` output of every example before and after, except `hello.br`), and a new test with `json` and `json_v2` fakes compiles with distinct types.
- [ ] 4.3 Merge the `.br` files of a package into one module, honor `_test.br`, report duplicates with both file names, check the `package` line. Verify: tests for a two-file package, a duplicate, a wrong `package` line and a `_test.br` file excluded from `-o` builds but run by `--test`.
- [ ] 4.4 Hand the checker the package of each declaration so `add-visibility-pub` can reject non-`pub` use across packages; if that change has not landed, leave a documented hook and record the scenario as pending. Verify: with `add-visibility-pub` present, the "private item" scenario test passes; otherwise the task notes it explicitly.
- [ ] 4.5 `src/lsp.rs` and `src/testrun.rs` call `resolve_project`. Verify: `tests/lsp.rs` completion/diagnostics for a dependency name in a project fixture, and `tests/testrun.rs` runs tests of a project with a dependency.

## 5. Repository content that uses `use`

- [ ] 5.1 Fix `examples/hello.br` (`use http, db, json` names nothing that exists) and any other source in `examples/`, `os/` and `std/` that `use`s a missing name. Verify: build every file in `examples/`, `os/` and `std/` and run `cargo test` (excluding `tests/async.rs` in routine runs, per project practice); no `unknown package` errors.
- [ ] 5.2 Add `examples/packages/` (a project with `brasa.toml`, `brasa.lock` pointing to a repository fixture, a local package, and a `main.br`) that builds offline from a cache the test prepares. Verify: `tests/package.rs` builds and runs it and compares the output.
- [ ] 5.3 Confirm `std/` and `os/` need no rewrite beyond 5.1 (their `use`s are all standard names) and record the confirmation in the PR. Verify: a grep of `^use` in `std/` and `os/` against the embedded name list in `std_source_for`.

## 6. Documentation

- [ ] 6.1 New `docs/packages.html` (manifest, lock, commands, cache, versions and majors, `use` resolution, visibility, git requirement and message, offline, `edition` reserved) with the navigation block shared by every page, linked from `docs/index.html`. Verify: `tests/docs.rs` (shared navigation, internal links and anchors).
- [ ] 6.2 `docs/getting-started.html`: document the new options. Verify: `tests/docs.rs` `documented_flags_match_the_command_line`.
- [ ] 6.3 README feature row and roadmap line; if `examples/packages/` is embedded in a page, regenerate with `brasa-parse --docs`. Verify: `tests/docs.rs` `embedded_examples_match_the_example_files`.

## 7. Final checks

- [ ] 7.1 Full test suite (async left out of routine runs) and `cargo clippy` clean for the new module; confirm no new crate in `Cargo.toml`. Verify: command output.
- [ ] 7.2 `openspec validate add-package-manager --strict`.
