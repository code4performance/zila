## Purpose

Share Zila Lang code between projects without a registry: a manifest and lock file, dependencies that are git repositories pinned by commit and hash, minimal version selection, a local cache, offline builds, and the rules by which `use` finds a package.

## Requirements

### Requirement: The manifest
A project SHALL be described by `zila.toml` at its root, written in a strict subset of TOML: tables (`[a]`, `[a.b]`), string values, comments and blank lines. `[package]` SHALL accept `name`, `version` (semver) and `edition`; each `[dependencies.<name>]` SHALL have `git` (a repository URL) and `version` (`MAJOR.MINOR.PATCH`, the minimum acceptable). `edition` SHALL be optional and SHALL be stored without being interpreted before 1.0. Any other key or construct (arrays, inline tables, multi-line strings) SHALL be an error that names the line and the construct.

#### Scenario: A valid manifest
- **WHEN** `zila.toml` has a `[package]` table and one `[dependencies.json]` table with `git` and `version`
- **THEN** the project loads and `use json` refers to that dependency

#### Scenario: Unsupported TOML
- **WHEN** `zila.toml` contains `names = ["a", "b"]`
- **THEN** the tool reports the line and that arrays are not supported, and does nothing else

#### Scenario: Edition is reserved
- **WHEN** `[package]` contains `edition = "2027"`
- **THEN** the manifest loads and the value does not change how the code compiles

### Requirement: Dependencies are git repositories pinned by commit
A dependency SHALL come from a git repository, with no central registry. Its module path SHALL be the URL without scheme, user and `.git` suffix. Versions SHALL be the repository's tags `vMAJOR.MINOR.PATCH`. `zila.lock` SHALL record, for every package in the selected graph, its version, the full commit its tag points to, and a `sha256:` tree hash, sorted so that equal graphs give equal files.

#### Scenario: Locking
- **WHEN** `--fetch` succeeds for a dependency at tag `v1.2.0`
- **THEN** `zila.lock` has an entry for `<module path>@1.2.0` with the 40-character commit of that tag and its tree hash

#### Scenario: A moved tag
- **WHEN** the locked tag now points to a different commit and `--fetch` runs
- **THEN** the tool reports that the tag moved, names both commits, keeps the lock unchanged and exits with a failure

### Requirement: Minimal version selection and major versions
For each module path the selected version SHALL be the highest of the minimum versions required by the project and, transitively, by the manifests of the selected packages; a newer version SHALL NOT be chosen merely because it exists. No search or backtracking SHALL take place. A module path ending in `/vN` (N at least 2) SHALL be a different package from the same repository without the suffix, SHALL use only tags with major N, and SHALL have its own import name, so two majors can be used in one program with distinct types.

#### Scenario: Two requirements on one package
- **WHEN** the project requires `lib` at 1.2.0 and a dependency requires `lib` at 1.4.1, and tags up to 1.9.0 exist
- **THEN** version 1.4.1 is selected and locked

#### Scenario: Majors coexist
- **WHEN** the manifest has `json` (module `.../json`, 1.2.0) and `json_v2` (module `.../json/v2`, 2.0.0)
- **THEN** both are fetched and locked, `use json` and `use json_v2` both work, and a value of one package's type is not accepted where the other's is expected

### Requirement: Commands
`zila-parse` SHALL accept `--init`, `--add <git-url>[@version]` (with `--as <name>`), `--fetch`, `--tidy`, `--update [name]` and `--offline`, which act on the project containing the current directory and need no source file. `--add` SHALL append the dependency to the manifest and then fetch. `--fetch` SHALL resolve the graph, download what the cache lacks, verify the hashes and write the lock. `--tidy` SHALL remove dependencies that no source uses and rewrite the lock to exactly the needed graph, without network access. `--update` SHALL raise minimums to the newest tag of the same major and fetch. With `--offline`, no command SHALL contact a remote, and one that needs to SHALL fail naming what is missing.

#### Scenario: Adding a dependency
- **WHEN** `zila-parse --add https://example.org/x/json@1.2.0` runs in a project
- **THEN** `zila.toml` gains `[dependencies.json]` with that URL and version, the code is in the cache and `zila.lock` lists it

#### Scenario: Tidy
- **WHEN** a manifest declares `json` and no source file uses it, and `--tidy` runs
- **THEN** the dependency is removed from `zila.toml` and from `zila.lock`

#### Scenario: Offline fetch with a missing package
- **WHEN** `--fetch --offline` runs and a locked package is not in the cache
- **THEN** the tool names the package and fails without contacting any remote

### Requirement: Git is used for fetching only
Fetching SHALL run the `git` executable found on `PATH`, with fixed arguments and never through a shell, and no other part of the toolchain SHALL run it. Prompts for credentials SHALL be disabled. When `git` cannot be started the tool SHALL print `error: git was not found on PATH. It is used only to download dependencies (zila-parse --fetch); building from an already populated cache does not need it.` and exit with a failure. A build from a populated cache SHALL NOT need `git`.

#### Scenario: git is missing
- **WHEN** `--fetch` runs on a machine where `git` is not on `PATH`
- **THEN** the tool prints the message above and exits with a failure

#### Scenario: Build without git
- **WHEN** the lock and cache are complete and `git` is not on `PATH`
- **THEN** `zila-parse prog.br -o prog` builds normally

### Requirement: Cache and offline builds
Fetched sources SHALL be stored under `$ZILA_HOME` (default `~/.zila`) in one immutable directory per package version and commit, shared by projects and written atomically. A build SHALL read only `zila.lock` and the cache, SHALL NOT use the network, and SHALL fail with `package 'X' is not in the cache; run zila-parse --fetch` when an entry is missing.

#### Scenario: Offline build
- **WHEN** the project has been fetched once and the network is then unavailable
- **THEN** the build succeeds with the same output

#### Scenario: Missing cache entry
- **WHEN** the cache directory of a locked package has been deleted and a build runs
- **THEN** the build fails with the message that names the package and suggests `--fetch`

### Requirement: Integrity on every build
Every build that uses a package SHALL recompute the SHA-256 tree hash of its cache directory and compare it with `zila.lock`, and SHALL refuse to build on a mismatch, naming the package and both hashes. A build SHALL also refuse when `zila.lock` does not cover the manifest.

#### Scenario: Tampered cache
- **WHEN** a file in a cached package is edited and a build runs
- **THEN** the build fails with a message that shows the expected and found hashes and suggests `--fetch`

#### Scenario: Stale lock
- **WHEN** a dependency is added to `zila.toml` by hand and a build runs before `--fetch`
- **THEN** the build fails saying `zila.lock` is out of date

### Requirement: Resolving `use`
For `use a.b.c`, the first segment SHALL be looked up among the core standard library, the project's declared dependencies, the local package directories next to `zila.toml` and the standard libraries outside the core; a name SHALL exist in only one of the first three, and only the core standard names SHALL be reserved. A dependency or local package whose name equals a standard library outside the core SHALL shadow it, and the build SHALL print a warning naming both. Later segments SHALL select a subdirectory of the package, and the name bound in code SHALL be the last segment. A package SHALL be the `.br` files directly in its directory, merged into one module; a duplicate declaration across them SHALL be an error naming both files; files ending in `_test.br` SHALL be left out of builds. A dependency's own `use` SHALL be resolved with its own manifest. A name found nowhere SHALL be an error that suggests `--add`.

#### Scenario: Dependency and sub-package
- **WHEN** `use json.stream` is written and `json` is a locked dependency with a `stream/` directory
- **THEN** `stream.<name>` refers to the declarations of `stream/*.br`

#### Scenario: Reserved name
- **WHEN** `--add` would give a dependency the import name `text`, a core standard library
- **THEN** the tool refuses, saying the name belongs to the core standard library, and changes nothing

#### Scenario: Shadowing a non-core library
- **WHEN** a dependency is named `json` and `json` is also a standard library outside the core
- **THEN** `use json` refers to the dependency and the build prints a warning that it shadows the standard one

#### Scenario: Unknown name
- **WHEN** a program has `use nothing` and no manifest, local directory or standard library of that name
- **THEN** compilation fails with `unknown package 'nothing'` and the `--add` hint

#### Scenario: Different versions below
- **WHEN** two dependencies each `use json` and their manifests select different majors
- **THEN** each is compiled against its own, with no symbol clash

### Requirement: Visibility across packages
Only `pub` declarations of a package SHALL be usable from another package (the rule of the `pub` modifier); the standard library follows the same rule.

#### Scenario: A private item
- **WHEN** a program uses a declaration of a dependency that is not `pub`
- **THEN** compilation fails naming the declaration and its package

### Requirement: Builds without a manifest are unchanged
A source file with no `zila.toml` above it SHALL compile exactly as before, with only the standard library available, except that unknown package names are errors.

#### Scenario: Single-file program
- **WHEN** `examples/hello.br` is built in a directory tree with no manifest
- **THEN** it builds and runs as before
