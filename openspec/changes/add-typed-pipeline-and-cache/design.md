## Context

`src/codegen_x64.rs` and its modules in `src/codegen_x64/` check and emit in one pass: `Compiler` methods resolve types, instantiate generics, bind calls and write assembly text together, with state in one large struct (`Compiler`, `FnState`, `Snapshot` for speculative compilation). `language-design-review` already gave the pass reachability pruning (`src/prune.rs`), a link report that assembles the output, and the module split. Packages are loaded by `load` into `Module { name, file }` values, and `package.rs` already hashes package trees (`tree_hash`) for the lock.

## Goals / Non-Goals

**Goals:**
- A typed program form that is the only input of emission.
- A cache of checked packages that never changes the output.

**Non-Goals:**
- Dynamic loading, a Zila Lang ABI, per-package machine code, parallel checking, a change of the executables.

## Decisions

1. **Introduce the typed form beside the existing pass, function by function.** Start with the declarations (records, enums, signatures, globals: `declare` and `layout` already produce tables) as a `TypedProgram` value, then bodies. At each step emission reads the new form for what it covers and the suite plus byte-identical assembly of every example is the check. A flag-day rewrite is rejected: the generator is too large to rewrite without an oracle.
2. **Cache the checked form, not machine code.** Generic instantiations are decided by the whole program (a package's `List<T>` code depends on which `T` its users ask for), so per-package machine code is not independent. The checked declarations and the bodies of non-generic functions are. Machine code is still produced on every build from the cached typed form; pruning and layout of shared symbols run over it.
3. **Key** = SHA-256 of the package's source tree (the same hash the lock uses), the keys of the packages it imports, the compiler version (a hash of the compiler's own build, so a change of the checker invalidates everything), the target and the optimisation flag. Entries live under `$ZILA_HOME/check/<key>` and are written atomically like the package cache.
4. **A damaged or missing entry is a miss**, never an error. The cache has a format version; an unknown version is a miss.
5. **Serialisation** is a plain, versioned binary or text form written by the compiler itself (zero external dependencies). The typed form is built so that it can be written and read without pointers into the source tree.
6. **The cache is off for `--run` of one-off files and for the language server** until measured; the default is on for builds of a project with a manifest. (Revisit once there are numbers.)

## Risks / Trade-offs

- The typed form must carry everything emission needs; a missed piece shows as a difference in output, which the byte-identical check catches at once.
- Serialising the form can cost more than checking small packages. Measure first on the larger libraries (`postgres`, `sqlserver`, `ai`) and keep the cache only where it wins; the requirement is about correctness, not speed.
- Two forms of the same information (the old tables and the typed form) exist during the migration. Keep the migration short per area and delete the old path as each area is done.
- Comptime generators run during checking and their results are already cached on disk (`--no-comptime-cache`); the key must include whatever they read, or the generator cache must be consulted first.

## Open Questions

- Which parts of `Compiler`'s state belong in the typed form and which are emission state (labels, temporaries, register use)? The answer shapes the first step and is settled by reading what `declare`, `layout` and `func_sig` already return.
