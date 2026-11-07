## Context

`Compiler::load` in `src/codegen_x64.rs` starts from the main `File`, collects `use` names (first segment only, `file.uses: Vec<Vec<String>>`), and for each name asks `std_source_for(name, target)` for an embedded `std/<name>.br` (or `std/<name>.linux.br`), parses it and queues it. Any other name is silently ignored (`examples/hello.br` has `use http, db, json` and compiles). Modules are keyed by their `package` name. `src/cli.rs` has a single `FLAGS` list that the parser and `tests/docs.rs` both follow; `src/main.rs` requires a source path for every mode but `--docs` and `--lsp`. The compiler has no dependencies (GOVERNANCE principle 9).

## Goals / Non-Goals

**Goals**
- Reproducible builds: the same manifest and lock give the same sources, checked by hash.
- Zero crates: manifest parser, semver, SHA-256 and the resolver are written in Rust inside the repository.
- A deterministic version choice that a person can compute by hand.
- Building never needs the network once the cache is filled.

**Non-Goals**
- Registry, publishing, signing, workspaces, build scripts, C sources in dependencies, platform-specific dependencies.

**Depends on**
- `add-visibility-pub`: the rule "only `pub` items cross a package boundary" is enforced by the checker that change adds. This change only gives the checker the package identity of every declaration. Until that change lands every item is visible, and the visibility scenario is verified when it does.
- `remove-interpreter`: the resolver feeds the native pipeline (and `--test`, the language server, `--fmt-check`). Nothing here uses `Interpreter`; while the interpreter still exists, `--run` does not resolve dependencies and says so.
- `add-direct-pe-output`: independent code, but together they make `brasa-parse` build with no external tool at all; `git` is needed only to fetch. The docs say so.
- `add-json`, `add-web`: the first expected users, as `std/` libraries or as packages; this change works either way. Standard names are reserved (decision 6).
- `add-comptime-reflection`, `add-copy-on-write`, `add-error-kind`, `add-text-bytes-and-char`, `add-weak-refs`, `add-derived-eq-ord-hash`: library authors rely on them; no interaction, and no scope duplicated here.

## Decisions

1. **Manifest `brasa.toml`, a strict TOML subset.** Alternatives: a Brasa-flavored syntax (new lexer rules and grammar to maintain and document, no editor support) or JSON (no comments, painful to write by hand). The subset needs a line parser of about 100 lines: `[table]` and `[table.name]` headers, `key = "string"` (strings only), `#` comments and blank lines. No arrays, inline tables, multi-line strings, or escapes beyond `\"` and `\\`. Anything else is an error with the line number that names the unsupported construct. It is valid TOML, so editors highlight it and a later move to a full parser breaks nothing.
   ```toml
   [package]
   name = "myapp"
   version = "0.1.0"

   [dependencies.json]
   git = "https://github.com/someone/json"
   version = "1.2.0"
   ```
   The table name (`json`) is the **import name**: `use json`. `[package] name` is the name other projects see; for an application it is informational. Unknown keys are errors, except `edition` (decision 9).
2. **Module path and versions.** A dependency's module path is its git URL without scheme, user and `.git` suffix (`github.com/someone/json`). Versions are semver `MAJOR.MINOR.PATCH` and git tags are `vMAJOR.MINOR.PATCH`. A dependency's `version` is a **minimum**, like Go's `require`. A major version 2 or higher is a different package: its module path ends in `/v2` (`github.com/someone/json/v2`, the same repository, tags `v2.*`) and the manifest gives it its own import name (`--add` defaults to `json_v2`), so `json` and `json_v2` can both be used, even through dependencies of dependencies, and their types are different types. A tag whose major does not match the module path suffix is ignored. Major 0 is one package like any other; the author may break it between minors and resolution does not know about that.
3. **Minimal version selection.** The requirement graph is built from the project's manifest and, recursively, from the `brasa.toml` of every package at the version it is required at. For each module path (which includes the major) the selected version is the **highest of the minimums** required anywhere in the graph, never a newer one. There is no backtracking and no solver, so the result depends only on the manifests: adding a package never upgrades another beyond what some manifest asked for. `--update` is the only thing that raises a minimum (it edits the manifest to the newest tag of that major).
4. **Lock file `brasa.lock`.** The same TOML subset, with tables named `[package."<module path>@<version>"]` holding `commit`, `hash` and `needs` (a string with the `<module path>@<version>` entries it depends on, comma-separated, informational). `commit` is the full 40-hex commit of the tag; `hash` is `sha256:<hex>` of the tree (decision 10). The lock lists the full graph, sorted, so diffs are small. Builds read it; `--fetch` and `--tidy` write it.
5. **Cache.** `$BRASA_HOME` if set, else `%USERPROFILE%\.brasa` on Windows and `$HOME/.brasa` elsewhere. Layout: `src/<module path>@<version>-<first 12 of commit>/` holding the files without `.git`. One immutable directory per version and commit, shared between projects. Writes go to a temporary sibling and are renamed into place, so an interrupted fetch never leaves a half package.
6. **Resolution of `use`.** For `use a.b.c` the first segment `a` is looked up in three places: (1) the **standard library** (`std/`); (2) the project's **dependencies** (`[dependencies.a]`); (3) a **local package**, a directory `a/` next to the manifest. A name may exist in only one place: the standard names are reserved, so `--add`, the manifest parser and local-package lookup reject a name that equals one, and the error names both. The remaining segments are directories inside the package (`use json.stream` is `<package root>/stream/`), and the name bound in code is the last segment, as for standard libraries today (`json.parse`, `stream.next`). A dependency's own `use` is resolved with **its** manifest, never the importing project's, so two packages can import different `json`s. A name found nowhere is a compile error: `unknown package 'a' (not in the standard library, brasa.toml or the project directory); to add one: brasa-parse --add <git-url>`. This replaces silently ignoring unknown names.
7. **A package is a directory.** All `.br` files directly in the package directory (subdirectories are sub-packages) form one module: declarations and `use`s are merged, a duplicate name is an error naming both files, and each file's `package` line must equal the package name (for a dependency, the last path segment of its manifest's `[package] name`; for a local package, the directory name). Files ending in `_test.br` are ignored by builds and used by `--test`. Module identity inside the compiler is `<module path>@<major>` (local packages: the directory path), and assembly symbols are mangled from it, so `json` and `json_v2`, or one package imported under two names, cannot collide. This requires `Compiler::load` to key modules (`self.modules`, `self.imports`, `cur_mod`, qualified names) by identity instead of `package` name; the qualifier in user code stays the import name.
8. **Visibility.** Across a package boundary only `pub` items are visible (`add-visibility-pub`). The standard library is already a separate package, so for it this is the existing rule. Inside a package nothing changes.
9. **Edition.** `[package] edition` is optional, a string, parsed and kept, and **not interpreted** before 1.0 because no edition exists to choose. It is reserved so that 1.0 can make it required and let the compiler apply per-package rules (GOVERNANCE.md, compatibility). `--init` omits it before 1.0, and the docs say why.
10. **Integrity.** The tree hash is SHA-256 over, for each file sorted by path (bytes, `/` separators, regular files, `.git` excluded): `path`, a zero byte, the decimal length, a zero byte and the contents. SHA-256 is implemented in `src/package.rs` (about 60 lines) and tested against the standard vectors. Every build that uses a package recomputes the hash of its cache directory and compares it with the lock. A mismatch is `error: package 'X' in the cache does not match brasa.lock (expected sha256:..., found sha256:...); run brasa-parse --fetch to restore it`. If the lock does not cover the manifest: `error: brasa.lock is out of date; run brasa-parse --fetch`.
11. **Commands**, all in `src/cli.rs` `FLAGS` (documented in `docs/getting-started.html`, which `tests/docs.rs` checks), acting on the project that holds the current directory (nearest `brasa.toml` upward) and taking no source file. `src/main.rs` handles them before the "source path required" check, as it does `--docs` and `--lsp`:
    - `--init [name]`: writes `brasa.toml` if absent.
    - `--add <git-url>[@version]`: resolves `version` (default: the highest tag of the highest major matching the URL's path), appends `[dependencies.<name>]`, then does what `--fetch` does. `--as <name>` sets the import name (default: the last path segment of the URL, with `_vN` for a `/vN` suffix).
    - `--fetch`: resolves the graph (decision 3), clones what the cache lacks, verifies the hashes of what it has, writes `brasa.lock`. `--offline` limits it to the cache and fails naming what is missing.
    - `--tidy`: removes manifest dependencies that no source `use`s, and rewrites `brasa.lock` to exactly the needed graph. Offline.
    - `--update [name]`: raises the minimum of `name` (or of all) to the newest tag of its major, then `--fetch`.
    A normal `brasa-parse prog.br -o prog` (and `--test`, `--asm`, `--fmt-check`, `--lsp`) finds the manifest above the source file, reads the lock, loads from the cache and **never** touches the network. Without a manifest the behavior is today's (standard library only), except that unknown names now error (decision 6). `--offline` is accepted for builds and has no effect there.
12. **Git, the only external program.** The compiler stays dependency-free in Rust. Fetching shells out to the `git` executable on `PATH`, through `std::process::Command` with fixed argument vectors (never a shell string; the URL follows `--`), **only** from the fetch code path (`--add`, `--fetch`, `--update`); nothing else in the toolchain may run it. Commands: `git ls-remote --tags -- <url>` (tag to commit; for annotated tags the peeled `^{}` commit), `git clone --quiet --no-checkout -- <url> <tmp>`, `git -C <tmp> checkout --quiet --detach <commit>`, then `.git` is removed before the tree is moved into the cache. `GIT_TERMINAL_PROMPT=0` is set so a private repository fails instead of waiting for a password; ssh agents and credential helpers work as the user configured them. URL forms: `https://`, `ssh://`, `git@host:path`, and `file://` (used by tests). If `git` cannot be started: `error: git was not found on PATH. It is used only to download dependencies (brasa-parse --fetch); building from an already populated cache does not need it.` Other git failures print `error: git failed while fetching <url>:` and git's own message. If the locked tag now points elsewhere: `error: tag vX of <url> moved (locked <a>, now <b>); the lock is kept. Run --update to accept it`.
13. **Offline builds.** Builds read only the lock and the cache, so a machine with a copied `~/.brasa/src` (or `BRASA_HOME` pointing to a shared directory) builds with no network and no git. A missing cache entry says: `package 'X' is not in the cache; run brasa-parse --fetch`.
14. **Language server and test runner** call the same resolver entry point (`package::resolve_project(source_path)`) so completion and `--test` see dependencies; the server never fetches.

## Risks / Trade-offs

- A moved tag is detected by the lock, but history can be rewritten or a repository deleted. With no registry there is no mirror; a shared `BRASA_HOME` or a vendored copy is the answer for now. A registry or proxy can later be another source of the same `module path@version -> tree` mapping without changing manifests or locks.
- Merging files into one module (decision 7) and re-keying modules touches many sites that use `package` names. It is the largest piece of work, so tasks 4.1 to 4.3 split it into testable steps.
- MVS means a fix in a new patch is not picked up until a minimum is raised (`--update`). That is the intended trade for predictability.
- A TOML subset that rejects valid TOML (arrays, inline tables) may surprise people; the error says which construct is unsupported.
- Hashing the tree on every build costs milliseconds for source-only trees; if it ever matters a per-project stamp file can skip it.
- A local package and a dependency cannot share a name; moving code between the two means renaming.

## Migration

A program with no manifest keeps compiling unless it `use`s a name that exists nowhere. `examples/hello.br` (`use http, db, json`) is changed by the task list; building every example verifies there are no others.
