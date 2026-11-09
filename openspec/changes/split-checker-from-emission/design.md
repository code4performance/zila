## Context

`add-typed-pipeline-and-cache` (archived) classified the state of `Compiler` (its design.md, "Findings") and moved the declaration tables into `TypedProgram`. The blocker for the rest is that expression and statement methods return `Ty` and push assembly in the same call, and `Snapshot` rolls back type tables and emitted text together (inference, checking a generic body).

## Goals / Non-Goals

**Goals:** a typed form of generics results, call bindings and function bodies that is the only input of emission; byte-identical assembly at every step.

**Non-Goals:** the package cache, new syntax or options, changes to the executables.

## Decisions

1. **Oracle.** The `--asm` output of every `examples/*.zila` and `examples/packages/*.zila` for the default build, `--target linux` and `--no-opt` (228 files), compared with a snapshot taken before each step, plus the suite without `tests/async.rs`. The output is deterministic since the first step, so any difference is a real change. A script that takes and compares the snapshots is added to `tools/` so the check is repeatable.
2. **Order.** Generics results and call bindings first (state moves like the declarations did), then bodies. A checker that types without emitting is introduced for one expression form at a time, with the old method delegating to it, so both paths agree while the migration lasts.
3. **Speculative compilation.** `Snapshot` is replaced by checking without emission where the checker covers the case; it stays where inference still needs to emit.

## Follow-up: the cache of checked packages

Once bodies have a typed form that carries no pointers into the source tree, a later change can cache it. What the first change recorded, kept here so it is not lost:

- Key: hash of the package's source tree (the lock's `tree_hash`), the keys of the packages it imports, the compiler version, the target and the optimisation flag; entries under `$ZILA_HOME/check/<key>`, written atomically; a damaged, missing or unknown-version entry is a miss. Not cached: machine code (generic instances are decided by the whole program). `--no-cache` turns it off. Comptime generator results must be part of the key or consulted first. Measure on `postgres`, `sqlserver` and `ai` before turning it on by default.
- Requirement for `static-linking`, to be added then:

```
### Requirement: Incremental builds give identical output
When the cache of checked packages is used, a build SHALL produce an executable byte-identical to the build with the cache disabled (`--no-cache`), and SHALL check a package again whenever its source, its dependencies' sources, the compiler version, the target or the flags change. A package SHALL NOT be checked again when none of them changed.

#### Scenario: Warm cache
- **WHEN** a project is built twice without changes, the second time with a warm cache
- **THEN** both executables are identical and the second build checks no package again

#### Scenario: Changed dependency
- **WHEN** one source file of a dependency changes
- **THEN** that package and the packages that use it are checked again and the others are taken from the cache

#### Scenario: The cache is only an optimisation
- **WHEN** the cache directory is deleted, or a cached entry is damaged
- **THEN** the build succeeds with the same executable, checking the packages again

#### Scenario: Other flags are other entries
- **WHEN** the same package is built with `--no-opt`, and for `--target linux`
- **THEN** each build uses its own cache entry and none returns the result of another
```

## Findings

**How a call is resolved today (task 2.2).** `Compiler::call` (`calls.rs`, about 650 lines) is a chain of cases that resolve the callee and emit code in the same arm. Two kinds of callee exist:

- *By name alone:* `f(...)`, `module.f(...)`, enum variants, generic functions and globals holding a closure. The target is found from `cur_mod`, the `funcs`, `tmpl_funcs`, `variants` and `globals` tables and the import aliases; nothing is emitted to learn it (a few built-ins such as `sleep` and `channel` are special-cased by name first). These can be bound without emitting.
- *By the type of the receiver:* `x.m(...)`. The target is `methods[(type key, m)]`, or `std_sig` for built-in types, or a field holding a closure, and the type of `x` is only known by calling `self.expr(obj)`, which emits the code of `x`. Binding these without emitting needs the checker of task 3.1 (typing an expression without emitting), so they cannot be bound before it exists.

So task 2.2 splits in two: name-based calls can be bound now, receiver-based calls only after 3.1.

**Step 3.1, first forms (type_of).** `src/codegen_x64/checker.rs` holds `type_of(&self, expr, want) -> Option<Ty>`, which types literals, identifiers (locals, globals, constants, functions as values), record field access and calls of functions declared by name (`f(x)`, `module.f(x)`), text literals and interpolations, comparisons, `and`, `or` and `not` without emitting; everything else answers `None`. `Compiler::expr` asserts in builds with debug assertions (all tests) that emission gives the same type. Running it on the examples found two real divergences, both the same cause: the built-in conversions (`int(...)`, `text`, other integer type names) win over a user function with the same name in the calling package (for example `db.int` called as `int(n)` inside `db`). They are excluded from `type_of`. Not covered yet: arithmetic and other operators, indexing, lists, maps, records, lambdas, `if`/`match`, methods, generics, `await`, `try`, `new`, `shared`. The check proves agreement only on code the suite compiles.
