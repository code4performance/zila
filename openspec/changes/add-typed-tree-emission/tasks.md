## 1. Typed Tree Definition and Checker Expansion

- [x] 1.1 `src/codegen_x64/typed.rs`: define `TypedExpr`, `TypedExprKind`, `TypedStmt`, `TypedStmtKind`, and `TypedBody` holding resolved `Ty`, variable bindings, and explicit call targets; verify unit tests build and pass with `cargo test --lib`
- [x] 1.2 `src/codegen_x64/checker.rs`: expand expression typing into a full `check_expr` producing `TypedExpr` across literals, arithmetic/bitwise/boolean binary ops, records, enums, calls, field access, index, and closures; verify `type_of` coverage across the test suite
  - Done: every expression form that is typed without entering a block or inferring type arguments, including `try` and variadic calls; emission asserts its type in debug builds. Not here, moved to 2.1 because they need the statement checker, scopes or instantiation: `if`, `match`, closures, `spawn_thread` and `sql` (both rewritten into statements by emission), and generic calls.

## 2. Pre-Emission Checking and Call Resolution

- [ ] 2.1 `src/codegen_x64/checker.rs` and `generics.rs`: add the forms moved from 1.2 (`if`, `match`, closures, `spawn_thread`, `sql`, generic calls) and type-check function bodies and concrete generic instances ahead of emission, attaching the resolved `TypedBody` to `TypedProgram`; verify all test targets compile
  - Progress: `check_function_body` (`src/codegen_x64/bodies.rs`) checks bodies before emission in debug builds (own scopes, no slots; the generic-instantiation state is restored after annotations are resolved) and stores a `TypedBody` per symbol in `TypedProgram::bodies`. Covered statements: `let`/`var`, assignment, `return`, expression statements, `if`, `while`, `for` (range, list, map, text), `match` as a statement (enum, `ok`/`err`, catch-all names), `unsafe`, `break`/`continue`; closures are typed when the expected function type is given; `.len` and `.code`, `push`/`add`/`clear` on lists, `has` on maps and `slice` on texts, methods declared on built-in types, integer conversions, `return` of an error in a fallible function, and globals of other modules are typed. The rest is `Opaque` and marks the body `complete: false`. Emission asserts the type of every checked `let`/`var`. Remaining: `match` and `if` used as values, closure bodies, `defer`/`spawn`/`asm`, `async` bodies, `spawn_thread`, `sql`, generic calls and typed bodies for generic instances, other built-in methods, `??`/optional forms; about half of the bodies in the larger examples are complete (`json` 85 of 143, `errors` 101 of 158, `taskbox` 7 of 7).
- [ ] 2.2 `src/codegen_x64/expressions.rs` and `calls.rs`: refactor expression emission to consume `TypedExpr` directly, reading pre-resolved types and call target symbols; verify `cargo test --test codegen` passes
  - Progress: calls of functions by name and enum-variant constructors are emitted from the typed node (`emit_resolved_call`: the target the checker resolved, no second lookup of the callee; visibility of a function of another module is still checked). `tools/asm-snapshot.py compare` against the snapshot taken before the change: identical (40 programs, 3 variants). Remaining: method calls, built-in calls, operators, field/index, literals and the other forms.

## 3. Statement Emission and Clean-up

- [ ] 3.1 `src/codegen_x64/statements.rs`: refactor statement emission to consume `TypedStmt`, removing speculative `Snapshot` rollbacks during body emission; verify `cargo test --test statements` and `--test run` pass
- [ ] 3.2 Clean up redundant type checking and dead `unreachable!()` branches across `src/codegen_x64/`; verify `cargo clippy --all-targets -- -D warnings` is clean

## 4. Verification and Wrap-up

- [ ] 4.1 Run `python tools/asm-snapshot.py compare` against baseline snapshots across all `examples/*.zila` to verify byte-identical assembly output across Windows, Linux, and non-optimized targets
- [ ] 4.2 Record in `CHANGELOG.md` under Internal that emission consumes the typed tree (no user-facing docs under `docs/` need modification as syntax and runtime behavior are unchanged); verify `cargo test --test docs` passes
- [ ] 4.3 Full test suite verification (`cargo test`) and archive the change with `openspec archive`
