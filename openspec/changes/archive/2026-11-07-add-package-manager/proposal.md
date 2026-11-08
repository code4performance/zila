## Why

Brasa programs can only `use` the libraries embedded in the compiler (`std/`), and a program is a single source file. There is no way to share code between projects, which blocks the ecosystem the language needs (JSON and web frameworks are planned as libraries). GOVERNANCE.md already says the package manager depends on foundational decisions (visibility, memory and error models) and that projects declare an `edition` from 1.0. This change adds the package manager now, while breaking changes are still allowed, with the smallest design that works: dependencies are git repositories pinned by commit, versions are semver with minimal version selection, and there is no registry.

## What Changes

- A **manifest** `brasa.toml` (a small, strict subset of TOML) at the project root: `[package]` (name, version, `edition` reserved) and one `[dependencies.<name>]` table per dependency (`git`, `version`).
- A **lock file** `brasa.lock`, written by the tool and committed: for every selected package its module path, version, git commit and a SHA-256 tree hash.
- **Commands** added to `brasa-parse`, in the style of `--fmt` and `--test`: `--init`, `--add <git-url>[@version]`, `--fetch`, `--tidy`, `--update [name]`, plus the modifiers `--as <name>` and `--offline`. Only `--add`, `--fetch` and `--update` use the network. A normal build never does.
- A **cache** outside the project (`$BRASA_HOME`, default `~/.brasa`) holding one checkout per package version and commit, verified by hash on every build; builds work offline from it.
- **Resolution**: `use name.path` finds `name` among the standard library, the project's dependencies and its local packages, with the rules the spec defines. A package is a directory of `.br` files; only `pub` items are visible to other packages.
- **Versions**: semver with Go-style minimal version selection (the smallest version that satisfies every requirement; no solver). A different major version is a different package with its own import name, so majors coexist.
- **Git** is the one external program the toolchain may run, for fetching only, with an explicit error when it is missing.
- Standard library, `os/`, examples, tests and docs that depend on `use` resolution are updated; a new documentation page describes packages.

Non-goals: a registry, publishing, package signing, building C sources of a dependency, workspaces.

## Capabilities

### New Capabilities

- `package-manager`: manifest, lock file, commands, cache, version selection, resolution of `use`, integrity and offline builds.

### Modified Capabilities

None. (The `use` of standard libraries is unchanged; what is added is resolution of non-standard names.)

## Impact

- New `src/package.rs` (manifest and lock parsing, semver, minimal version selection, SHA-256, git runner, cache, resolver); `src/cli.rs` (`FLAGS`, `Mode`) and `src/main.rs` (dispatch, project discovery, error reporting); `src/codegen_x64.rs` (`Compiler::load` consults the resolver where `std_source_for` finds nothing; module identity includes the package); `src/lsp.rs` and `src/testrun.rs` (use the same resolver so the editor and `--test` see dependencies).
- New `tests/package.rs` (uses local `file://` git repositories, skips without `git`), `docs/packages.html`, `docs/getting-started.html` (the option list checked by `tests/docs.rs`), the navigation of every docs page, README, an `examples/packages/` sample project.
- Breaking: a `use` of a name that is neither standard, declared nor local now fails with a message that suggests `--add`. `examples/hello.br` relies on the old behavior (`use http, db, json`) and is fixed by this change.
