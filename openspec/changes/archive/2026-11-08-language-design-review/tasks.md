## 1. Phase 1: investigate

- [x] 1.1 Find why unused libraries are not pruned (measured: there was no pruning at all) and record the cause in design.md
- [x] 1.2 List the `unreachable!()` sites, mark which ones user code can reach (none found: guarded sites, and a mutation run of 2,945 variants gave no panic; see design.md)

## 2. Phase 1: pruning and inert modules

- [x] 2.1 Make top-level state and OS import registration part of the reachability graph (initialised only when read)
- [x] 2.2 Implement reachability-based emission for every library
- [x] 2.3 Add tests (`tests/prune.rs`): an unused `use x` for each std library leaves the program unchanged on Windows and for the main ones on Linux; a used function and what it calls are kept; `use ui` unused makes no window import; initialisers with effects are kept; `--no-prune` keeps everything
- [x] 2.4 Run the full test suite (all tests are allowed, including async) with pruning on and fix regressions; keep an opt-out flag until stable

## 3. Phase 1: tooling

- [x] 3.1 Add `--explain-link` (`src/cli.rs`, `src/main.rs`, `link_report` in the code generator), measured on the assembled program, for Windows and Linux; test in `tests/prune.rs`
- [x] 3.2 Add the `std/X.br` vs `std/X.linux.br` parity test (`tests/std_parity.rs`) as a ratchet over `tests/data/std_parity_known.txt` (61 known differences: internal `pub` helpers and Windows-only functions; none in the API a program uses)
- [x] 3.3 Package effects (`src/effects.rs`): shown by `--explain-link` for the reachable code and by `--add`, `--fetch` and `--update` for new or changed packages, with the CHANGED mark; tests in `src/effects.rs` and `tests/package.rs`
- [x] 3.4 `[policy] forbid` in the manifest subset and its enforcement at build, kept by `--add`; tests in `tests/package.rs`

## 4. Phase 1: documentation

- [x] 4.1 Docs: `packages.html` (what goes into the executable, effects, policy), `native.html`, `getting-started.html` (`--no-prune`, `--explain-link`); verified with `cargo test --test docs`
- [x] 4.2 No example needed for `--explain-link`: the docs check passes with the flag documented in `getting-started.html` and `tests/prune.rs` runs it; nothing in `examples/` changes

## 5. Phase 2: small std and official packages

- [x] 5.1 Write the core std list into `std_source` as data (core vs non-core) and update `is_reserved` to reserve only core names
- [x] 5.2 Implement shadowing of non-core std names by dependencies and local packages, with the warning, and tests for both scenarios in the delta spec
- [x] 5.3 Move the libraries to `packages/<name>/` (with their `zila.toml`, Linux layers in `linux/`), delete the `std/` copies and embed from `packages/`; each builds as a local package (`tests/packages.rs`)
- [x] 5.4 No deprecation warning: nothing outside the repository uses Zila Lang (user decision, 2026-11-08); using a non-core library as `std` keeps working with no message
- [x] 5.5 Update `docs/stdlib.html`, `docs/packages.html`, `docs/databases.html`, `docs/ai.html` and the README: which libraries are core, which are official packages, the deprecation; verify with `cargo test --test docs`
- [x] 5.6 Record the open question (hosting, manifest-less builds) in the proposal for when someone outside the repository uses Zila Lang

## 6. Phase 3: compiler structure (the typed form and the cache moved to the change `add-typed-pipeline-and-cache`)

- [x] 6.2 `src/codegen_x64.rs` split into `src/codegen_x64/` modules (analysis, calls, drop, expressions, fold, generics, layout, lifecycle, memory, output, runtime, statements, threads); the main file went from 10,750 to about 2,100 lines, with no change in behaviour (the whole suite is the check)
- [x] 6.3 No `unreachable!()` site is known to be reachable (task 1.2); the dead `&& false` branch under `StmtKind::Return` is deleted
