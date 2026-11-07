## Why

The grammar and the docs promised explicit generic arguments at call sites (`first<int>(x)`), but the parser rejected them. Inference cannot work when no argument or expected type determines a type parameter (for example `empty<int>()`), so such functions could not be called at all.

## What Changes

- The parser accepts `f<T, U>(args)` and `module.f<T>(args)`. The `<...>` is read as type arguments only when there is no space before `<` and none between `>` and `(`; with spaces, `a < b, c > (d)` stays a comparison.
- `ExprKind::Call` carries the explicit type arguments; the code generator binds them first and infers the remaining parameters. More type arguments than parameters is a compile-time error.
- `docs/syntax.html` no longer lists the feature as unimplemented. `html ... end` templates remain unimplemented.

## Capabilities

### New Capabilities

- `explicit-type-arguments`: calling generic functions with type arguments written at the call site.

### Modified Capabilities

None.

## Impact

`src/ast.rs`, `src/parser.rs`, `src/codegen_x64.rs`, `src/interp.rs`, `src/regalloc.rs`, `tests/generics.rs`, `tests/parse.rs`, `docs/syntax.html`. Written after the code (retroactive record).
