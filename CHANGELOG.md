# Changelog

All notable changes to Zila Lang are recorded here, newest first.
The format follows [Keep a Changelog](https://keepachangelog.com/) and versions follow [Semantic Versioning](https://semver.org/) (while below 1.0, minor versions may break compatibility).

The version lives in `Cargo.toml` and at the top of `README.md`. To release: move the entries from **Unreleased** under a new version heading, bump both places, and tag the commit (`git tag vX.Y.Z`).

## [Unreleased]

### Changed
- The command is now `zila` (before, its name ended in a suffix from the lexer-and-parser days); the VS Code extension starts `zila --lsp` by default (extension version 0.5.0).
- `zila.ebnf` and `zila-ffi.md` moved to `docs/`.
- The language's purpose is described more broadly in the README and the site: AI client and services, databases, web, IoT and mobile (the last two are targets), extensible through packages, open source.

### Fixed
- The README roadmap listed 15 changes as not started that were already built and archived, and a `rename-language` change that does not exist; it now lists only the real gaps. The test count is stated as "more than 1,000".
- The generated assembly is deterministic: two compiles of the same program no longer list the record and enum helper procedures in a different order.

### Added
- GitHub Actions workflow (`.github/workflows/ci.yml`): build, `cargo clippy -D warnings` and every test target except `async` on Windows and Linux, and the `async` tests as a separate job.
- `tests/docs.rs` checks that the README roadmap names only changes that are open in `openspec/changes/`, and says there are none exactly when there are none.

### Removed
- `docs/Zila.docx`, an old Word copy of the README that no longer matched it (the README is the one source; the file stays in the git history).

### Internal
- Clippy is clean with `-D warnings`: about 90 warnings fixed, and a few style lints that would make the code worse are allowed on the item with `#[allow]`.
- Declaration tables and generic instantiation results of the code generator live in `TypedProgram` (`src/codegen_x64/typed.rs`); `type_of` (`src/codegen_x64/checker.rs`) types expressions without emitting and is checked against emission in every test build; `tools/asm-snapshot.py` compares the compiler's assembly before and after an internal change. The typed tree that emission would consume, and after it the cache of checked packages, are later changes.

## [0.1.0] - 2026-11-08

First tagged state of the language, as built between 2026-11-05 and 2026-11-08.

### Language
- Readable syntax with `end`-closed blocks, static types, errors as values (`T or error`, `try`), `defer`, `abort` (replaces panic), `pub` visibility.
- Generics and interfaces with explicit type arguments, derived `eq`/`ord`/`hash`, comptime reflection, `int`/`long` aliases, floating point, maps, sets and sorted collections.
- Reference-counted memory with copy-on-write, weak references, restricted reference escape, `#[no_alloc]` and `#[allocator]`, raw `asm` blocks.
- Async execution without function coloring, threads and `shared`.

### Compiler and toolchain
- Native x86-64 compiler for Windows (direct PE output) and Linux, with register allocation, inlining and a basic optimizer. The interpreter was removed.
- `zila fmt`, `zila test`, package manager, language server and VS Code extension, documentation site.
- Bare-metal OS (kernel written in Zila).

### Standard library and packages
- Text, bytes and char, lists, typed `format`, buffered I/O, JSON, UDP, async DNS, TLS, web, native UI on Windows and Linux.
- Packages: `json`, `crypto`, `db`, `postgres` (and other drivers), `ai` client, `mcp`, `rag`.
