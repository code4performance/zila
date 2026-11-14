## Why

Code generation still traverses raw AST nodes while resolving types and binding calls on the fly during machine code emission. Although declarations live in `TypedProgram` and expression types are checked by `type_of` in test builds, the emission pass continues to recalculate types and carry speculative rollbacks (`Snapshot`). Having emission consume an explicit typed tree is the missing architectural foundation needed for incremental package compilation caching, advanced Language Server capabilities (accurate hover, definition, and diagnostics), and eliminating redundant typing and defensive `unreachable!()` branches in the compiler.

## What Changes

- **Typed Tree Representation**: Define an explicit typed representation for expressions and statements (carrying resolved types `Ty`, exact call targets, and resolved field/variable bindings) in `src/codegen_x64/typed.rs` or `typed_ast.rs`.
- **Pre-Emission Type Checking and Binding**: The checking phase (`checker.rs`) processes function bodies and generic instances, producing the typed tree before emission starts.
- **Emission Driven by Typed Tree**: `codegen_x64` expression and statement emission is refactored to read the typed tree directly without re-evaluating types or attempting speculative rollback.
- **Removal of Redundant Checks**: Deprecate duplicated type inspection and impossible error branches (`unreachable!()`) across `src/codegen_x64/expressions.rs` and `calls.rs`.
- **Assembly Regression Gate**: The compiler's generated assembly across all example programs and packages must remain byte-identical according to `tools/asm-snapshot.py`.

## Capabilities

### New Capabilities
None.

### Modified Capabilities
None. (`skip_specs: true` in `.openspec.yaml` for this internal compiler pipeline restructuring).

## Impact

- `src/codegen_x64.rs` and modules in `src/codegen_x64/` (`checker.rs`, `typed.rs`, `expressions.rs`, `statements.rs`, `calls.rs`, `generics.rs`).
- Zero changes to syntax, standard library behavior, or output executable ABI/semantics.
