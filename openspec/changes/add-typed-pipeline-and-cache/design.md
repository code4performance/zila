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

## Findings

**Which state of `Compiler` is checked information and which is emission state (task 1.1).** Read from `src/codegen_x64.rs` (`struct Compiler`, `Snapshot`, `generate`) and `layout.rs` (`load`, `declare`, `layout`, `func_sig`). `declare` fills the declaration tables in one pass over every module, `generate` then compiles each function body and the pending instances, and only after that prunes and writes text.

*Checked information (belongs in `TypedProgram`, step 1.2):*
- Type tables: `rec_decls`, `records` (`RecInfo`: size and field offsets), `enum_decls`, `enums` (`EnumInfo`), `variants`, `aliases`, `shared_recs`, `rc_recs`, `enum_payload`, `pub_types`, `interfaces`.
- Signatures and globals: `funcs`, `methods` (`Sig`), `globals` (`Global`), `derived`, `exported`, `await_syms`, `inline_bodies` (a decision from the body, with `opt`).
- Packages: `modules`, `imports`, `mod_aliases` (what `load` resolved).
- Generics: the templates (`tmpl_funcs`, `tmpl_recs`, `tmpl_methods`) are source and stay in the AST; the results (`inst_args`, `inst_funcs`, `pending`, `inst_count`, `map_thunks`) are checked information but are decided by the whole program, so they are the part of the typed form that cannot be cached per package (Decision 2).
- Extern facts: `extern_syms`, `extern_lib`, `callbacks`, `libs`, `uses_ui`, `uses_thread_lib`, `threads`, `custom_allocator`.

*Emission state (stays in the code generator, never cached):* `data`, `procs`, `strings`, `fn_closures`, `labels`, `fs` (`FnState`: `code`, `scopes`, frame slots, `loops`, temporaries, `defer_*`, `has_asm`, `may_own`), `win_imports`, `div_nonzero`, `inline_depth`, `next_lambda_share`, `next_lambda_ignore_result`, `cur_mod`, `line`, `bindings` (while one instance is compiled), and `deferred_bounds` / `deferred_keys` (checks waiting for `declared`).

*What blocks the split:* expression and statement methods return `Ty` and push assembly in the same call (`Compiler::expr` and its siblings), and speculative compilation (`Snapshot`: inference, checking a generic body) relies on rolling back both the type tables and the emitted text together. Steps 1.3 and 1.4 therefore need a checker that can type an expression without emitting, which today does not exist; every body-level `Ty` decision has to be lifted out of the 6,000 lines of `expressions`, `calls`, `statements`, `generics` and `memory`. The declaration tables alone (step 1.2) can be extracted first because `declare` already fills them before any emission.

**Step 1.2 done.** `src/codegen_x64/typed.rs` defines `TypedProgram`, which holds the declaration tables (`rec_decls`, `records`, `enum_decls`, `enums`, `variants`, `aliases`, `shared_recs`, `rc_recs`, `enum_payload`, `pub_types`, `interfaces`, `funcs`, `methods`, `globals`, `derived`, `exported`); `Compiler` has one field `tp` and emission reads everything through it. Generic instance results stay in `Compiler` until step 1.3. Check used for byte-identical output: the assembly (`--asm`) of every `examples/*.zila` and `examples/packages/*.zila`, for the default build, `--target linux` and `--no-opt` (228 files), compared with a snapshot taken before the change; plus the suite without `tests/async.rs`.

**Output was not deterministic before this change.** `generate` walked `records` and `enums` (hash maps) in an unspecified order, so two runs of the same compile produced the same lines in a different order. The incremental-build requirement (identical executables with and without the cache) cannot hold on top of that, so both loops now sort their keys. The snapshots above were taken after this fix, three runs identical. Other iterations over hash maps were checked by the same repeated-snapshot comparison, not by reading every site.
