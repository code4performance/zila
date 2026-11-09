## Why

The code generator checks types and emits assembly in one pass. `language-design-review` split its 10,750-line file into modules (analysis, calls, drop, expressions, fold, generics, layout, lifecycle, memory, output, runtime, statements, threads) but the pass is still one: every method that emits code also decides what the types are, which is why many sites re-derive a type and then `unreachable!()` on the case they just checked. It also means nothing can be cached between builds, because there is no intermediate form to cache: a build recompiles every package it uses, however small the edit. Two things are open as a result: a compiler structure that can grow, and builds that scale with what changed.

## What Changes

- **A typed program form.** The checker produces a typed program (types resolved, generics instantiated, calls bound to their targets, closures with their captures) and code generation only consumes it. Output assembly stays byte-identical for every program in the repository at each step.
- **A cache of the checked form** under `$ZILA_HOME`, keyed by the hash of a package's sources, the hashes of its dependencies, the compiler version, the target and the flags. A package whose key did not change is not checked again; the whole-program steps (pruning, layout of shared symbols, emission) run on every build over the cached results. `--no-cache` turns it off.
- The requirement that a build with the cache produces an executable byte-identical to one without it, and that a changed package and the packages that use it are the only ones re-checked.

## Capabilities

### New Capabilities
None.

### Modified Capabilities
- `static-linking`: adds the incremental-build requirement (output identical with and without the cache; only what changed is re-checked).

## Impact

- `src/codegen_x64/` (the checker output type and its consumers; the layout, calls, generics, expressions and statements modules are the main work), a new cache module, `src/cli.rs`/`src/main.rs` (`--no-cache`), `tests/` (byte-identical output for every example with and without the cache, re-check counts), `docs/packages.html`, `docs/native.html`.
- No syntax change and no change to the executables. Prerequisite already done in `language-design-review`: pruning, `--explain-link`, the module split and the list of `unreachable!()` sites (none known to be reachable).
- Non-goals: dynamic loading of Zila Lang packages, a stable Zila Lang-to-Zila Lang ABI, caching machine code per package (generic instantiations make per-package machine code depend on the program), and parallel checking.

## Outcome

Only the first part was done: the declaration tables moved into `TypedProgram`, and code generation became deterministic (it walked hash maps in an unspecified order, so two runs wrote the same lines in a different order, which made byte-identical output impossible to check). The rest of the typed form moved to `split-checker-from-emission` and the cache is a later change, because the cache needs a typed form of function bodies that does not exist yet. The incremental-build requirement was dropped from the `static-linking` delta (it is kept in the design of `split-checker-from-emission`); the delta now holds the requirement this change did deliver, deterministic output, with its test.
