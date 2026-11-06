## Why

Generics and interfaces are in the grammar, the parser and the docs (`func first<T>(items: []T)`, `type Stack<T>`, `T: Printable`), but the native compiler silently skips generic functions and cannot use generic types. Anyone who writes reusable code (a `Stack<T>`, `show<T: Printable>`) has to copy it per type, and the standard library cannot grow generic helpers. It is item 1 on the roadmap now that floating point is done.

## What Changes

- **Generic functions**: `func first<T>(items: []T) -> T?` compiles. Each distinct set of type arguments produces its own copy of the function (monomorphization). Type arguments are **inferred** from the call's arguments; a call that leaves a parameter undetermined is a compile error naming it.
- **Generic records**: `type Stack<T> = record ... end` and `Stack<int>` as a type in variables, parameters, returns, fields, lists and maps. Each instantiation is its own record type. Methods declared with a generic receiver, `func (s: Stack<T>) push(x: T)`, work on every instance.
- **Interfaces**: `type Printable = interface func to_text(self) -> text end` declares method signatures. A type satisfies an interface **structurally**: it has a method with each name and identical parameter and result types. No `implements` clause.
- **Bounds**: `func show<T: Printable>(x: T)` accepts only types that satisfy `Printable` and may call its methods on `x`. Violations are compile errors naming the type, the interface and the missing or mismatching method.
- **Errors at the definition**: using a method, field or operator on an unbounded `T` that the body needs is reported once, at the generic definition.
- **Interpreter**: `--run` runs generic programs (it is dynamically typed and ignores type parameters).
- Docs, `examples/generics.br`, tests, and the README status table and roadmap are updated in the same change.

## Capabilities

### New Capabilities
- `generics`: generic functions and records, type arguments and inference, instantiation, generic methods, errors.
- `interfaces`: interface declarations, structural satisfaction and bounds on type parameters.

### Modified Capabilities
<!-- None: non-generic code behaves as before. -->

## Impact

- **Compiler** (`src/codegen_x64.rs`): templates for generic functions and records, type substitution in `resolve`, an instantiation queue, inference at call sites, interface satisfaction checks, name mangling for instances.
- **Parser** (`src/parser.rs`): none expected; generics, bounds and interface declarations already parse. Explicit call-site arguments (`first<int>(x)`) stay out of scope because they clash with comparison syntax.
- **Interpreter** (`src/interp.rs`): accept generic declarations and test them.
- **Docs, examples, tests**: `docs/syntax.html`, `docs/native.html`, `docs/examples.html`, `README.md`, `examples/generics.br`, new `tests/generics.rs`.
- **Assumptions** (no clarification needed):
  - Interfaces are used **only as bounds**. A parameter, field or variable whose type is an interface needs dynamic dispatch (vtables) and is rejected with a message pointing at bounds. A good follow-up change.
  - Instances share no code; code size grows with the number of distinct instances.
  - Inference uses call arguments and, for a parameter that appears only in the result, the expected type of the call. No explicit type arguments at call sites.
  - Generic enums, generic type aliases, async generic functions, generic closures and generic `extern` functions give a clear error.
  - A type parameter has at most one bound (the grammar allows one `qname`).
  - There are no built-in numeric bounds, so `a + b` on `T` is an error unless written for concrete types.
