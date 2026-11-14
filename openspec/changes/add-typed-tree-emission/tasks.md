## 1. Typed Tree Definition and Checker Expansion

- [x] 1.1 `src/codegen_x64/typed.rs`: define `TypedExpr`, `TypedExprKind`, `TypedStmt`, `TypedStmtKind`, and `TypedBody` holding resolved `Ty`, variable bindings, and explicit call targets; verify unit tests build and pass with `cargo test --lib`
- [ ] 1.2 `src/codegen_x64/checker.rs`: expand expression typing into a full `check_expr` producing `TypedExpr` across literals, arithmetic/bitwise/boolean binary ops, records, enums, calls, field access, index, and closures; verify `type_of` coverage across the test suite
  - Progress: `check_expr` covers every expression form that has no block inside: literals, text, identifiers, operators, field, index, lists, maps, non-generic record literals, `new`, `shared`, `^`, `await`, enum variants (as values and as constructors), calls of named functions and record methods with arguments bound to their parameters (`TypedArg`), and the built-ins `print`, `println`, `error`, `abort`, `char`, `float`/`f64`/`f32`, `sleep`, `live_objects`; emission asserts its type in debug builds. Remaining: forms with blocks (`if`, `match`, closures) and the statement tree (see 2.1), `try`, `spawn_thread`, `sql`, generic calls, variadic calls.

## 2. Pre-Emission Checking and Call Resolution

- [ ] 2.1 `src/codegen_x64/checker.rs` and `generics.rs`: type-check function bodies and concrete generic instances ahead of emission, attaching the resolved `TypedBody` to `TypedProgram`; verify all test targets compile
- [ ] 2.2 `src/codegen_x64/expressions.rs` and `calls.rs`: refactor expression emission to consume `TypedExpr` directly, reading pre-resolved types and call target symbols; verify `cargo test --test codegen` passes

## 3. Statement Emission and Clean-up

- [ ] 3.1 `src/codegen_x64/statements.rs`: refactor statement emission to consume `TypedStmt`, removing speculative `Snapshot` rollbacks during body emission; verify `cargo test --test statements` and `--test run` pass
- [ ] 3.2 Clean up redundant type checking and dead `unreachable!()` branches across `src/codegen_x64/`; verify `cargo clippy --all-targets -- -D warnings` is clean

## 4. Verification and Wrap-up

- [ ] 4.1 Run `python tools/asm-snapshot.py compare` against baseline snapshots across all `examples/*.zila` to verify byte-identical assembly output across Windows, Linux, and non-optimized targets
- [ ] 4.2 Record in `CHANGELOG.md` under Internal that emission consumes the typed tree (no user-facing docs under `docs/` need modification as syntax and runtime behavior are unchanged); verify `cargo test --test docs` passes
- [ ] 4.3 Full test suite verification (`cargo test`) and archive the change with `openspec archive`
