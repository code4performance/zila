## 1. Foundations

- [x] 1.1 Generalize the method table so receivers can be any type (`func (n: int) to_text() -> text`), not only records; verify with a program calling a method on an `int` and on a `text`
- [x] 1.2 Keep generic function and record declarations as templates (module, declaration, type parameters) instead of skipping them, and report unsupported forms (generic enums, aliases, async, closures, extern) by name; verify with compile-error tests for each form

## 2. Interfaces

- [x] 2.1 Record interface declarations, reject duplicate method names, and reject an interface used as a variable, parameter, field, element or result type with the `<T: Name>` suggestion; verify with compile-error tests
- [x] 2.2 Implement structural satisfaction (`satisfies(type, interface)`) with the missing-method and mismatching-signature messages; verify with direct tests through bounds once task 3.3 exists, and with the declaration tests for now

## 3. Generic functions

- [x] 3.1 Add type substitution over `Type` nodes, the instance table and mangled names, and the instantiation queue with the depth limit; verify with a program calling `first<T>` on `[]int` and `[]text`, and with a mutually recursive generic pair
- [x] 3.2 Add type argument inference (unify parameters with argument types, then the return type with the expected type) and the undetermined and conflicting errors; verify with the inference scenarios of the spec and a test that argument expressions run exactly once
- [x] 3.3 Turn bounds into checks: verify each type argument against its bound at the call that supplies it; verify with satisfied, missing-method and mismatching-signature programs (a qualified bound from another module goes through the same lookup but has no test, since tests compile a single file)
- [x] 3.4 Check generic bodies once with opaque `Ty::Param` types (method calls through the bound, errors for other uses at the definition); verify with the unbounded method and operator scenarios
- [x] 3.5 Verify ownership: generic functions that build, return and store texts, lists, records and maps; verify with `live_objects()` over 1000 calls

## 4. Generic records and methods

- [x] 4.1 Resolve `Name<Args>` to an instantiated record (argument count check, field substitution, distinct instance types) and infer record literals from the expected type; verify with the instance, literal and argument-count scenarios
- [x] 4.2 Instantiate methods with the record (`func (s: Stack<T>) push(x: T)`), including methods with their own type parameters; verify with a `Stack<T>` of `int` and of `text`, and a bound on a record parameter (`Box<T: Printable>`)
- [x] 4.3 Verify generic records nested in each other and in lists, maps, closures and channels, with no leaks under `live_objects()`

## 5. Interpreter

- [x] 5.1 Make `--run` accept and run generic functions, records and interfaces; verify that `examples/generics.br` prints the same output natively and with `--run`

## 6. Documentation and examples

- [x] 6.1 Add `examples/generics.br` (a `Stack<T>`, a generic `first`, and `show<T: Printable>` with two record types) and its section on `docs/examples.html` (via `brasa-parse --docs`); verify it builds and prints the documented output
- [x] 6.2 Update `docs/syntax.html` (generics, inference, interfaces as bounds, what is not supported), remove generics and interfaces from the not-yet-supported list in `docs/native.html`, and update the status table and roadmap in `README.md`; verify `cargo test` passes, including the docs tests
