## Why

Code generation still checks types and emits assembly in one pass: the methods that emit code also decide what the types are, so many sites re-derive a type and then `unreachable!()` on the case they just checked, and there is no intermediate form that could be stored between builds. The first step is done (`add-typed-pipeline-and-cache`, archived): the declaration tables live in `TypedProgram` and assembly output is deterministic. What is left is the part that makes the form useful: generics instantiation, call binding and function bodies.

## What Changes

- **Generics and call binding** move into the checker output (`inst_args`, `inst_funcs`, `pending`, `map_thunks` and the target each call resolves to), and emission reads them.
- **Function bodies** get a typed form (expression and statement types, closures with their captures). A checker that types an expression without emitting code replaces the current methods that return `Ty` and push assembly in one call, including what `Snapshot` rolls back today.
- **The old path is deleted** area by area as the typed form covers it, with the `unreachable!()` sites it makes impossible.
- At every step the assembly of every program in the repository stays byte-identical (`--asm`, default build, `--target linux` and `--no-opt`), and the suite passes.

## Capabilities

### New Capabilities
None.

### Modified Capabilities
None. No behaviour, syntax or executable changes.

## Impact

- `src/codegen_x64.rs` and `src/codegen_x64/` (`expressions`, `calls`, `statements`, `generics`, `memory`, `lifecycle`, `drop`), `src/codegen_x64/typed.rs`, `src/derive_ops.rs`.
- Non-goals: the cache of checked packages (a later change, see the design), dynamic loading, a stable ABI, per-package machine code, parallel checking.
