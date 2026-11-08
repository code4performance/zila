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
- Deleting the domain libraries from the compiler (Phase 2 only publishes and deprecates; the deletion is a later breaking change).
- A central registry.

## Decisions

1. **Static, whole-program linking stays the model.** It matches "small executable, no runtime to install", keeps the lock/hash story simple (the build is the only consumer of dependency code) and avoids an ABI promise (generics, comptime and reference-count layouts would all become ABI). Dynamic packages are rejected until 1.0 fixes layouts. Alternative considered: shared libraries per package; rejected for the ABI cost and because it breaks "one file to ship".
2. **Pruning by reachability from roots**, computed before emission. Roots: `main`, C-exported functions, tests being run, allocator hooks. Top-level state is a node in the graph: it is initialised, and its OS imports registered, only if a reachable declaration reads it. Cause found (task 1.1): there was no pruning; `generate` emitted every non-generic function of every loaded module, ran every module initialiser, kept every C import, and switched on `ui` and `thread` behaviour by the `use` alone. Implemented in `src/prune.rs` over the generated text units: symbols are found by identifier; roots are `main`, exported functions, the custom allocator and the runtime; a module initialiser is kept only when what it reaches has an effect (a C call, a closure call, printing, `exit`, a system call, a runtime helper not on the pure list) or when kept code uses a global it can set; C imports, the windowing subsystem and atomic counts follow what is kept.
3. **`--explain-link` and `package-effects` read the same reachability result**, so reports cannot drift from the output. Effects are computed on reachable code of the package, so a never-called `extern` is not reported against the consumer.
4. **Effects are information first, policy second.** The policy is one optional `[policy] forbid = "..."` line because the manifest subset has no arrays; allowing a list later would be a manifest change of its own.
5. **The parity check is a test**: parse both files with the existing parser and compare `pub` signatures. A thinner per-platform `sys` layer, with `fs` and `net` written once, is the cleaner end state; it is a follow-up once parity is enforced, since changing structure and checking it at once hides regressions.
6. **Core std list** (Phase 2): `text`, `list`, `map`/`set`, `sorted`, `fs`, `net`, `sync`, `task`, `future`, `thread`, `math`, `float`, `errors`, `format`, `bufio`, `reflect`, `derive`, `sys`. Rule for membership: the compiler or the runtime depends on it, or every program needs it, or it has no external protocol to track. `json`, `crypto`, `tls`, `ui`, `web`, `ai`, `mcp`, `rag`, `db`, `postgres`, `sqlserver` track external formats and services, so they change faster than the compiler.
7. **Official packages live in this repository first** (a `packages/<name>/` directory with its own `brasa.toml`, usable by path as local packages and testable here), and move to their own git repositories when a release process exists. This avoids hosting decisions blocking the work. Assumption: the owner of this repository publishes them under one organisation; if the hosting choice differs, only the URLs in the docs change.
8. **Deprecate before deleting.** In Phase 2 using a domain library as `std` keeps working and prints a warning naming the package and how to `--add` it. Removal needs its own change and a release in between.
9. **Shadowing only for non-core names**, with a warning, so adding a library to std never breaks a package that already had the name, and core names stay unambiguous in every program.
10. **Compiler split** (Phase 3): the checker returns a typed program (types resolved, generics instantiated, calls bound); emission takes only that. Sites that are `unreachable!()` today because the generator re-derives types disappear with it; the ones that remain are real invariants. Do the negative-test list (task 1.2) in Phase 1 so the split has a safety net.
    Result of task 1.2: the 15 `unreachable!()` sites in `src/codegen_x64.rs` are each guarded by a check just before them (a `matches!` on the same type, an operator already matched, a type the compiler built itself). A mutation run over `examples/*.br` (2,945 variants: each line deleted, plus substitutions of types, `ref`, `let`/`var` and `end`) gave no compiler panic. None is known to be reachable from user code, so no negative test exists for them and task 6.3 only covers a site that a later run finds. One dead branch (`&& false` under `StmtKind::Return`) can never run and can be deleted.
11. **Object cache** (Phase 3): key = hash of the package sources, the hashes of its dependencies, the compiler version and the target and flags. Whole-program steps that cannot be cached (pruning, layout of shared symbols) run on every build over cached per-package results. If per-package results cannot be made independent of the rest of the program (generic instantiations are the likely obstacle), cache the checked form instead of the machine code. The requirement is only that output is identical with and without the cache.

## Risks / Trade-offs

- Pruning bugs drop needed code (initialisers, `defer`, drop glue, derives reached only through interfaces). Mitigation: the existing suite must pass unchanged, and an opt-out flag exists while stabilising.
- Interface dispatch and comptime-expanded code make reachability conservative, so some bytes stay. Accepted.
- Size assertions can be brittle: compare against the hello size of the same build, not absolute numbers.
- Splitting std costs the "batteries included, no manifest needed" experience. Mitigation: the deprecation phase, and `--add` with the official URL, keep the first run to one command; whether to keep a bundled copy for manifest-less builds is the open point below.
- Phase 3 is large and touches every code path of the generator. Mitigation: phase gating, and the whole test suite as the oracle (output must not change).

## Open Questions

- Should a program with no `brasa.toml` keep seeing the domain libraries bundled in the compiler after the removal change, or must it have a manifest? This does not change Phases 1 to 3 and is decided in the removal change.
