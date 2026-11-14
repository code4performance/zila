## 1. Typed Tree Definition and Checker Expansion

- [x] 1.1 `src/codegen_x64/typed.rs`: define `TypedExpr`, `TypedExprKind`, `TypedStmt`, `TypedStmtKind`, and `TypedBody` holding resolved `Ty`, variable bindings, and explicit call targets; verify unit tests build and pass with `cargo test --lib`
- [x] 1.2 `src/codegen_x64/checker.rs`: expand expression typing into a full `check_expr` producing `TypedExpr` across literals, arithmetic/bitwise/boolean binary ops, records, enums, calls, field access, index, and closures; verify `type_of` coverage across the test suite
  - Done: every expression form that is typed without entering a block or inferring type arguments, including `try` and variadic calls; emission asserts its type in debug builds. Not here, moved to 2.1 because they need the statement checker, scopes or instantiation: `if`, `match`, closures, `spawn_thread` and `sql` (both rewritten into statements by emission), and generic calls.

## 2. Pre-Emission Checking and Call Resolution

- [x] 2.1 `src/codegen_x64/checker.rs` and `generics.rs`: add the forms moved from 1.2 (`if`, `match`, closures, `spawn_thread`, `sql`, generic calls) and type-check function bodies and concrete generic instances ahead of emission, attaching the resolved `TypedBody` to `TypedProgram`; verify all test targets compile
  - [x] 2.1a `bodies.rs`: `check_function_body` walks bodies before emission (own scopes, no slots) and stores a `TypedBody` per symbol in `TypedProgram::bodies`: `let`/`var`, assignment, `return`, expression statements, `if`, `while`, `for`, `unsafe`, `break`/`continue`; emission asserts the type of every checked `let`/`var`
  - [x] 2.1b `match` statements, closures whose function type is given, members and methods of built-in types, integer conversions, globals of other modules
  - [x] 2.1c `if` and `match` as values, `defer` and `spawn` statements
  - [x] 2.1d bodies of closures are walked with their own scopes (parameter and result types from the expected function type); emission asserts the type of their locals too
  - [x] 2.1e `async` bodies (checked as the closure they are compiled as; the type of each local is asserted there too)
  - [x] 2.1f generic calls (type arguments bound from the arguments, the instance signature built and forgotten again), methods of type-parameter bounds, calls of closures; the bodies of concrete generic instances are checked like any body
  - [x] 2.1g `spawn_thread` and inline `asm`
  - [x] 2.1i `sql` blocks are typed as the call of `db.sql_const` or `db.sql` they expand to
  - [x] 2.1h the statement forms still `Opaque` were taken by frequency until 4,891 of 5,684 bodies in the examples (86%) were complete; what is left is C interop (`c.*` intrinsics, `extern` calls with their pointer and integer coercions, the `ws`/`tls` wrappers over them)
- [ ] 2.2 `src/codegen_x64/expressions.rs` and `calls.rs`: refactor expression emission to consume `TypedExpr` directly, reading pre-resolved types and call target symbols; verify `cargo test --test codegen` passes
  - [x] 2.2a calls of functions by name and enum-variant constructors are emitted from the typed node (`emit_resolved_call`); `tools/asm-snapshot.py compare`: identical
  - [x] 2.2b method calls emit to the method the checker resolved; snapshot identical
  - [x] 2.2c calls of the simple built-in functions (`print`, `println`, `error`, `abort`, `char`) are emitted from the typed node; snapshot identical
  - [x] 2.2f calls of the conversions (`int(x)`, `u8(x)`, `float(x)`, `bool(x)`...), `sleep` and `live_objects` are emitted from the typed node; snapshot identical
  - [x] 2.2g calls of methods of a text or a char (`byte`, `bytes`, `char_count`, `slice`, `to_text`, `len_utf8`, `is_ascii`) are emitted from the typed node; snapshot identical
  - [ ] 2.2h calls of methods of lists and maps (copy-on-write places) and of `channel`, `weak`, `timeout`
  - [ ] 2.2d operators (arithmetic, comparison, boolean)
  - [ ] 2.2e field, index, literals and the remaining expression forms

## 3. Statement Emission and Clean-up

- [ ] 3.1 `src/codegen_x64/statements.rs`: refactor statement emission to consume `TypedStmt`, removing speculative `Snapshot` rollbacks during body emission; verify `cargo test --test statements` and `--test run` pass
  - [x] 3.1a the speculative typing of a `sql` hole, of the literal side of an operator and of an argument of a generic call uses the checker's type when the checker covers the expression; `tools/asm-snapshot.py compare --labels`: identical (label numbers shift, see design.md)
  - [ ] 3.1b statement emission reads the typed statement (the type of a `let`/`var` and of a condition) instead of deriving it again
  - [ ] 3.1c the speculative fallbacks that remain for forms the checker does not cover
- [ ] 3.2 Clean up redundant type checking and dead `unreachable!()` branches across `src/codegen_x64/`; verify `cargo clippy --all-targets -- -D warnings` is clean
  - [x] 3.2a the duplicate `type_of` is gone (`check_expr` is the one typing; emission asserts against it), and the repeated copy-or-retain code with its `unreachable!()` guards is one helper (`copy_or_retain`); clippy clean, `compare --labels` identical
  - [ ] 3.2b the typing emission still repeats for forms the tree does not feed it yet, and the remaining `unreachable!()` guards of invariants (they go with 3.1b and 3.1c)

## 4. Verification and Wrap-up

- [ ] 4.1 Run `python tools/asm-snapshot.py compare` against baseline snapshots across all `examples/*.zila` to verify byte-identical assembly output across Windows, Linux, and non-optimized targets
- [ ] 4.2 Record in `CHANGELOG.md` under Internal that emission consumes the typed tree (no user-facing docs under `docs/` need modification as syntax and runtime behavior are unchanged); verify `cargo test --test docs` passes
- [ ] 4.3 Full test suite verification (`cargo test`) and archive the change with `openspec archive`
