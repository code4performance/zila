## 1. Typed Tree Definition and Checker Expansion

- [x] 1.1 `src/codegen_x64/typed.rs`: define `TypedExpr`, `TypedExprKind`, `TypedStmt`, `TypedStmtKind`, and `TypedBody` holding resolved `Ty`, variable bindings, and explicit call targets; verify unit tests build and pass with `cargo test --lib`
- [x] 1.2 `src/codegen_x64/checker.rs`: expand expression typing into a full `check_expr` producing `TypedExpr` across literals, arithmetic/bitwise/boolean binary ops, records, enums, calls, field access, index, and closures; verify `type_of` coverage across the test suite
  - Done: every expression form that is typed without entering a block or inferring type arguments, including `try` and variadic calls; emission asserts its type in debug builds. Not here, moved to 2.1 because they need the statement checker, scopes or instantiation: `if`, `match`, closures, `spawn_thread` and `sql` (both rewritten into statements by emission), and generic calls.

## 2. Pre-Emission Checking and Call Resolution

- [ ] 2.1 `src/codegen_x64/checker.rs` and `generics.rs`: add the forms moved from 1.2 (`if`, `match`, closures, `spawn_thread`, `sql`, generic calls) and type-check function bodies and concrete generic instances ahead of emission, attaching the resolved `TypedBody` to `TypedProgram`; verify all test targets compile
  - [x] 2.1a `bodies.rs`: `check_function_body` walks bodies before emission (own scopes, no slots) and stores a `TypedBody` per symbol in `TypedProgram::bodies`: `let`/`var`, assignment, `return`, expression statements, `if`, `while`, `for`, `unsafe`, `break`/`continue`; emission asserts the type of every checked `let`/`var`
  - [x] 2.1b `match` statements, closures whose function type is given, members and methods of built-in types, integer conversions, globals of other modules
  - [x] 2.1c `if` and `match` as values, `defer` and `spawn` statements
  - [ ] 2.1d bodies of closures (walked with their own scopes)
  - [x] 2.1e `async` bodies (checked as the closure they are compiled as; the type of each local is asserted there too)
  - [ ] 2.1f generic calls and the bodies of concrete generic instances
  - [x] 2.1g `spawn_thread` and inline `asm`
  - [ ] 2.1i `sql` blocks (emission expands them into calls after typing each hole by a speculative compile)
  - [ ] 2.1h the statement forms that are still `Opaque` in the examples, by frequency (measured: 807 of 1,325 bodies complete)
- [ ] 2.2 `src/codegen_x64/expressions.rs` and `calls.rs`: refactor expression emission to consume `TypedExpr` directly, reading pre-resolved types and call target symbols; verify `cargo test --test codegen` passes
  - [x] 2.2a calls of functions by name and enum-variant constructors are emitted from the typed node (`emit_resolved_call`); `tools/asm-snapshot.py compare`: identical
  - [x] 2.2b method calls emit to the method the checker resolved; snapshot identical
  - [x] 2.2c calls of the simple built-in functions (`print`, `println`, `error`, `abort`, `char`) are emitted from the typed node; snapshot identical
  - [ ] 2.2f calls of the other built-ins (conversions, `sleep`, `channel`, `weak`, ...) and of methods of built-in types
  - [ ] 2.2d operators (arithmetic, comparison, boolean)
  - [ ] 2.2e field, index, literals and the remaining expression forms

## 3. Statement Emission and Clean-up

- [ ] 3.1 `src/codegen_x64/statements.rs`: refactor statement emission to consume `TypedStmt`, removing speculative `Snapshot` rollbacks during body emission; verify `cargo test --test statements` and `--test run` pass
- [ ] 3.2 Clean up redundant type checking and dead `unreachable!()` branches across `src/codegen_x64/`; verify `cargo clippy --all-targets -- -D warnings` is clean

## 4. Verification and Wrap-up

- [ ] 4.1 Run `python tools/asm-snapshot.py compare` against baseline snapshots across all `examples/*.zila` to verify byte-identical assembly output across Windows, Linux, and non-optimized targets
- [ ] 4.2 Record in `CHANGELOG.md` under Internal that emission consumes the typed tree (no user-facing docs under `docs/` need modification as syntax and runtime behavior are unchanged); verify `cargo test --test docs` passes
- [ ] 4.3 Full test suite verification (`cargo test`) and archive the change with `openspec archive`
