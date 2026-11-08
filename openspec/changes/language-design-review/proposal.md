## Why

Brasa grew from a parser into a compiler, a 22-library standard library, a package manager, an LSP and a small OS in about a week of changes. One question that decides a lot of the design has never been written down: **where does a package's code live in the final program** — inside the executable, or loaded later? The answer today is true but implicit, and it has side effects (binary size, std release coupling, a 10,000-line code generator) that deserve a decision before 1.0. This change records the answer, reviews the design for anti-patterns, and plans the fixes in order.

## Findings (what is true today)

**Packaging model.** Everything is compiled in; nothing is loaded dynamically.
- The standard library is embedded in the compiler: `std/*.br` are pulled in with `include_str!` through a name table in `src/codegen_x64.rs` (`std_source`, plus `std_linux_source` for the Linux layers). `use json` compiles `json.br` together with your program; the output executable contains the result and needs no `std` directory at run time.
- Third-party packages are git repositories of `.br` source, fetched into `~/.brasa`, hash-checked against `brasa.lock`, and compiled **from source on every build** into the same single executable (whole-program compilation). There is no compiled-library format, no `.dll`/`.so` package, no plugin loading.
- The only run-time linking is `extern` FFI: the PE import table (`kernel32`, `ucrt`, `user32`, ...) resolved by the OS loader, and the Linux equivalent. That is C code, not Brasa packages.
- Measured with an up-to-date release build of `brasa-parse` before this change: a hello world is 6,656 bytes, and a `use` that is never called still costs the whole library (`use json` 96,768 B, `use tls` 172,544 B, `use web` 323,072 B, `use ai` 371,712 B, `use rag` 431,104 B). There was no pruning at all: the code generator emitted every non-generic function of every loaded module. (An earlier draft of this proposal said `json`, `crypto` and `postgres` were pruned; that came from a stale release binary and was wrong.) The rule was neither specified nor tested.

**Criticisms and anti-patterns found.**
1. **The linking model is unspecified.** What ends up in the binary is accidental; the `tls`/`ui` size jump shows it.
2. **Std is welded to the compiler.** A library fix or a new library needs a compiler release; users cannot pin or replace a std library. `ai`, `rag`, `mcp`, `postgres`, `sqlserver`, `ui`, `web`, `tls`, `crypto`, `json` are domain libraries that fit the package manager better than the compiler. Reserved std names also mean every new std library can break an existing third-party package of that name.
3. **Two parallel std trees.** `*.linux.br` duplicates module surfaces by hand; nothing checks that both expose the same `pub` items.
4. **No separate compilation.** Every build recompiles every dependency; fine now, a wall for large projects.
5. **Monolithic code generator.** `src/codegen_x64.rs` is about 10,400 lines (44% of `src/`) and mixes type checking with emission, with many `let ... else { unreachable!() }` that would turn a type-checker gap into a compiler panic instead of a diagnostic.
6. **Feature breadth outran hardening.** Threads, async, comptime, inline asm, an OS, two DB drivers and a RAG library landed before 1.0 semantics (memory model, reference-count cycles beyond `weak`, ABI stability) are specified.
7. **Dependency trust.** Hash pinning is good, but a dependency's use of `unsafe`, `extern` and `asm` is not visible to the consumer, and comptime code runs at build time.
8. **Module load had effects.** Every module's initialiser ran at start-up, `use ui` switched the program to the windowing subsystem and `use thread` made every reference count atomic, whether or not anything of the library was used.
9. **Hand-rolled infrastructure** (TOML subset, SHA-256, git subprocess) fits the zero-dependency rule, but the unit tests are the only guard.

Not verified in this review: runtime performance, the correctness of the reference-count and `shared` semantics, and the `unreachable!()` sites one by one (task 1.2).

## What Changes

The work is in three phases, each shippable on its own. Phases 2 and 3 start only after the previous one is merged.

**Phase 1 — Specify and fix the linking (no breaking change)**
- Specify the model: all packages (std and third party) are statically compiled into one executable; no dynamic loading of Brasa packages; `extern` is the only run-time linking.
- Loading a module has no effects: initialisation of a package's top-level state runs only when something reachable needs it.
- Unreachable-declaration pruning for every library, so `use x` costs bytes only for what is reachable (fixes `tls`/`ui`).
- `brasa-parse <file> --explain-link`, listing the packages and the bytes each contributes.
- A `std/X.br` vs `std/X.linux.br` surface parity test.
- `--fetch` and `--explain-link` show, per package, whether it uses `unsafe`, `extern` or `asm` (the **effects** of a package).

**Phase 2 — Small std, official packages (BREAKING only when removal lands, a later change)**
- Define the **core std**: `text`, `list`, `map`/`set`, `sorted`, `fs`, `net`, `sync`, `task`, `future`, `thread`, `math`, `float`, `errors`, `format`, `bufio`, `reflect`, `derive`, `sys`.
- Publish the domain libraries (`json`, `crypto`, `tls`, `ui`, `web`, `ai`, `mcp`, `rag`, `db`, `postgres`, `sqlserver`) as official packages, each with its own `brasa.toml`, version and tests, and add a deprecation warning when they are used as `std`. Removing them from the compiler is a separate later change.
- Only core std names are reserved; a dependency may shadow any other name with a warning.

**Phase 3 — Compiler structure and incremental builds**
- Split type checking from code generation: the checker produces a typed intermediate form and code generation only consumes it. Move `src/codegen_x64.rs` into modules (layout, calls, generics, drop, derive). Replace the `unreachable!()` sites reachable from user code with diagnostics.
- A compiled-object cache keyed by source hash, so unchanged packages are not recompiled.

## Capabilities

### New Capabilities
- `static-linking`: what a program contains (all code from `use`d packages, compiled in, pruned to what is reachable), that loading a module has no effects, what is never loaded at run time, how a build reports it, and platform-layer parity.
- `package-effects`: the report of what a package does (`unsafe`, `extern`, `asm`) shown to the consumer.

### Modified Capabilities
- `package-manager`: the rule for reserved names (core std only) and shadowing with a warning.

## Impact

- Phase 1: `src/codegen_x64.rs` (reachability, initialisation), `src/cli.rs`, `src/main.rs`, `src/fetch.rs` (report), `tests/`, `docs/packages.html`, `docs/native.html`.
- Phase 2: a `packages/` area or separate repositories for the official packages, `src/package.rs` (reserved names, shadowing), `std_source` table (deprecation warning), `docs/stdlib.html`, `docs/packages.html`, `README.md`.
- Phase 3: `src/codegen_x64.rs` split into modules, new checker output type, cache under `$BRASA_HOME`.
- No syntax change. Output binaries become smaller or equal. The one breaking step (deleting the domain libraries from the compiler) is deliberately out of this change.
