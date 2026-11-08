# Changelog

All notable changes to Zila Lang are recorded here, newest first.
The format follows [Keep a Changelog](https://keepachangelog.com/) and versions follow [Semantic Versioning](https://semver.org/) (while below 1.0, minor versions may break compatibility).

The version lives in `Cargo.toml` and at the top of `README.md`. To release: move the entries from **Unreleased** under a new version heading, bump both places, and tag the commit (`git tag vX.Y.Z`).

## [Unreleased]

### Added
- Typed pipeline and cache (`openspec/changes/add-typed-pipeline-and-cache`, in progress).

### Changed
- `zila` command rename (`openspec/changes/rename-command-to-zila`, in progress).

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
