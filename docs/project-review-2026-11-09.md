# Zila Lang: open items from the 2026-11-09 review

This file keeps **only what is still open** from the review made at commit `c7401ab`. What was resolved was removed from here and is recorded in the OpenSpec change [`2026-11-09-repo-hygiene-and-ci`](../openspec/changes/archive/2026-11-09-repo-hygiene-and-ci) and in `CHANGELOG.md` (Unreleased section).

Resolved (summary, details in the change): README roadmap rewritten and guarded by a test, test count, `rename-language` removed, tag `v0.1.0`, CI workflow, clippy clean with `-D warnings`, `docs/Zila.docx` removed, how to recreate `.claude`/`.agent`/`.opencode` documented, `threads` spec marked as not met on Linux, README shortened and the AI-assisted, human-directed development stated openly.

## Checks still to do

- **First CI run on GitHub**, mainly the Linux job: it was written without a runner available, and some test targets may assume Windows and need a skip guard.
- **`tests/async.rs`** was not run in this round (it is skipped in routine runs); the CI `async` job will cover it.

## Open items by area

### Compiler

1. **Unfinished typed pipeline.** `type_of` is only checked against emission in test builds. Missing: emission consuming a typed tree, recording the target of each call, removing the duplicated typing and the `unreachable!()` sites it makes impossible, and only then the package cache (incremental builds). Keep `tools/asm-snapshot.py` as the gate.
2. **Audit of `unwrap()`** (120 in `src/`): separate those in tests from those on production paths.
3. **A `SAFETY:` comment policy** for the 49 uses of `unsafe` in `src/`.

### Platform

4. **Linux thread layer** (`clone`, `futex`, a per-thread scheduler on `epoll`), and weak references and identity on `shared T`. The `threads` spec already says this requirement is not met.
5. **Windows/Linux `std` parity**: `tests/data/std_parity_known.txt` has 63 lines, mostly in `task`; the list may only shrink.
6. `target/` is 7.7 GB: consider `debug = "line-tables-only"` in the dev profile or a periodic `cargo clean`.
7. New targets (macOS, ARM64, WebAssembly) only after the typed pipeline and a stable CI; ARM64 is the biggest lever for "IoT and mobile".

### Tooling

8. **Language server**: hover, go to definition, completion and type errors (`type_of` is the natural base).
9. **VS Code**: build the `.vsix` and do the manual smoke test.
10. **TLS**: server side, `close_notify`, a buffered reader.

### Security and trust

11. **Threat model and `SECURITY.md`**: comptime runs code at build time; packages come from git (the hash pins the source, but `effects.rs` only shows `unsafe`/`extern`/`asm`); a reporting channel.
12. **Hardening**: official vectors (NIST/RFC) for `crypto` and TLS (it was not checked whether `tests/crypto.rs` uses them), fuzzing of the lexer, parser and comptime, an idempotence property for the formatter, tests for the PE/ELF writers.
13. The `web` server has no TLS, HTTP/2, cookies or multipart: acceptable for development, and documented.

### Process and scope

14. **Freeze new libraries** until the foundations are hardened: memory semantics and `shared` in writing, a 1.0 breaking-change policy, an exit checklist for pre-alpha. In four days async, threads, comptime, asm, the OS, TLS, UI, web, AI, MCP, RAG, two databases, SQL blocks, the language server and the extension all landed.
15. **Bus factor**: a single apparent author; `GOVERNANCE.md` describes roles that do not exist in practice yet. Open "good first issue" issues from the README's table of gaps, now that the README asks for volunteers.
16. **Docker vs local servers**: `tests/docker/` coexists with tests against local servers; document which path is supported and how CI (which has no databases) behaves.

### Clarity

17. Decide one rule for "Zila" vs "Zila Lang" (the README now uses "Zila Lang" in prose, the command is `zila`).
18. A **compiler architecture document** (lexer, parser, checks, comptime, prune, codegen, PE/ELF) in `docs/`, today scattered across the archived `design.md` files; and a `docs/README.md` indexing `zila.ebnf`, `zila-ffi.md`, `diagrams.md` and this file.
19. Of the clippy lints allowed with `#[allow]` on the item (parallel indices, `too_many_arguments` on three collectors in `noalloc.rs`, `to_text`), re-evaluate whether a small refactor removes them.

## State verified after the fixes

`cargo clippy --all-targets -- -D warnings` clean; every test target except `async`: 1,017 passed, 0 failed, 1 ignored (the new roadmap test included). After the README rewrite, `tests/docs.rs` (21 tests) passes.
