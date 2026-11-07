## 1. Parser and compiler

- [x] 1.1 `targs` on `ExprKind::Call`; parse `f<T>(...)` with the adjacency rule and an undoable speculative parse
- [x] 1.2 `call_generic` binds explicit types first and rejects extra ones; interpreter and register allocator ignore the field

## 2. Tests and documentation

- [x] 2.1 `tests/generics.rs`: explicit arguments, comparisons stay comparisons, too many arguments
- [x] 2.2 `docs/syntax.html`: the feature is no longer listed as unimplemented; verified by `cargo test --test docs --test generics`
