## Context

See proposal.md for the findings and the three phases. Constraints: zero external dependencies, own assembler and PE/ELF writers, a single-pass code generator in `src/codegen_x64.rs`, std embedded with `include_str!`, dependencies compiled from cached source (`src/package.rs`, `src/fetch.rs`). Measured before the change (up-to-date release build): hello 6,656 B; unused `use json` 96,768 B, `use crypto` 71,168 B, `use tls` 172,544 B, `use ui` 67,584 B, `use ai` 371,712 B. After Phase 1 pruning every one of them is 6,656 B.

## Goals / Non-Goals

**Goals:**
- A written, tested contract for what the executable contains.
- Pruning and module loading without effects, for all libraries.
- A small standard library, with domain libraries as versioned packages.
- A compiler whose type checking and emission are separate, and that can reuse compiled packages.

**Non-Goals:**
- Dynamic loading of Brasa packages or a stable Brasa-to-Brasa ABI.
- Any syntax change.
- A package registry, or hosting the official packages outside this repository.
- A central registry.

## Decisions

1. **Static, whole-program linking stays the model.** It matches "small executable, no runtime to install", keeps the lock/hash story simple (the build is the only consumer of dependency code) and avoids an ABI promise (generics, comptime and reference-count layouts would all become ABI). Dynamic packages are rejected until 1.0 fixes layouts. Alternative considered: shared libraries per package; rejected for the ABI cost and because it breaks "one file to ship".
2. **Pruning by reachability from roots**, computed before emission. Roots: `main`, C-exported functions, tests being run, allocator hooks. Top-level state is a node in the graph: it is initialised, and its OS imports registered, only if a reachable declaration reads it. Cause found (task 1.1): there was no pruning; `generate` emitted every non-generic function of every loaded module, ran every module initialiser, kept every C import, and switched on `ui` and `thread` behaviour by the `use` alone. Implemented in `src/prune.rs` over the generated text units: symbols are found by identifier; roots are `main`, exported functions, the custom allocator and the runtime; a module initialiser is kept only when what it reaches has an effect (a C call, a closure call, printing, `exit`, a system call, a runtime helper not on the pure list) or when kept code uses a global it can set; C imports, the windowing subsystem and atomic counts follow what is kept.
3. **`--explain-link` and `package-effects` read the same reachability result**, so reports cannot drift from the output. Effects are computed on reachable code of the package, so a never-called `extern` is not reported against the consumer.
4. **Effects are information first, policy second.** The policy is one optional `[policy] forbid = "..."` line because the manifest subset has no arrays; allowing a list later would be a manifest change of its own.
5. **The parity check is a test**: parse both files with the existing parser and compare `pub` signatures. A thinner per-platform `sys` layer, with `fs` and `net` written once, is the cleaner end state; it is a follow-up once parity is enforced, since changing structure and checking it at once hides regressions.
6. **Core std list** (Phase 2): `text`, `list`, `map`/`set`, `sorted`, `fs`, `net`, `sync`, `task`, `future`, `thread`, `math`, `float`, `errors`, `format`, `bufio`, `reflect`, `derive`, `sys`. Rule for membership: the compiler or the runtime depends on it, or every program needs it, or it has no external protocol to track. `json`, `crypto`, `tls`, `ui`, `web`, `ai`, `mcp`, `rag`, `db`, `postgres`, `sqlserver` track external formats and services, so they change faster than the compiler.
7. **Official packages live in this repository, as the only copy.** `packages/<name>/` has the library (`<name>.br`) and its `brasa.toml`; the compiler embeds the file with `include_str!`, so a program with no manifest still says `use json`. The directory is also a project, so the libraries can be built and tried as local packages. A Linux layer is `packages/<name>/linux/<name>.br` (a subdirectory, so it is not merged into the Windows package). Hosting them in other repositories is left for when someone outside this repository uses Brasa.
8. **No deprecation phase.** Nothing outside this repository uses Brasa, so language and library compatibility is not a constraint: the libraries were moved to `packages/` in one step, with no warning for using them as part of the standard library and no copy left in `std/`. Shadowing a non-core name still prints a warning, because that is the one case where the program does not mean what the compiler would otherwise guess.
9. **Shadowing only for non-core names**, with a warning, so adding a library to std never breaks a package that already had the name, and core names stay unambiguous in every program.
10. **Compiler split** (Phase 3): the module split was done here, mechanically, with the suite as the oracle. The typed form that emission would consume (types resolved, generics instantiated, calls bound) is the change `add-typed-pipeline-and-cache`; the sites that are `unreachable!()` only because the generator re-derives types disappear with it.
    Result of task 1.2: the 15 `unreachable!()` sites in `src/codegen_x64.rs` are each guarded by a check just before them (a `matches!` on the same type, an operator already matched, a type the compiler built itself). A mutation run over `examples/*.br` (2,945 variants: each line deleted, plus substitutions of types, `ref`, `let`/`var` and `end`) gave no compiler panic. None is known to be reachable from user code, so no negative test exists for them and task 6.3 only covers a site that a later run finds. One dead branch (`&& false` under `StmtKind::Return`) can never run and can be deleted.
11. **Object cache**: moved to `add-typed-pipeline-and-cache`, which caches the checked form (generic instantiations make per-package machine code depend on the whole program).

## Risks / Trade-offs

- Pruning bugs drop needed code (initialisers, `defer`, drop glue, derives reached only through interfaces). Mitigation: the existing suite must pass unchanged, and an opt-out flag exists while stabilising.
- Interface dispatch and comptime-expanded code make reachability conservative, so some bytes stay. Accepted.
- Size assertions can be brittle: compare against the hello size of the same build, not absolute numbers.
- Splitting std costs nothing for programs: the compiler still embeds the libraries, so no manifest is needed. What it buys is one directory per library that can be built, tested and versioned on its own.
- Phase 3 is large and touches every code path of the generator. Mitigation: phase gating, and the whole test suite as the oracle (output must not change).

## Open Questions

- Resolved: a program with no `brasa.toml` keeps seeing the libraries, because the compiler embeds them from `packages/`. Reopen it only if the libraries are ever versioned apart from the compiler.
