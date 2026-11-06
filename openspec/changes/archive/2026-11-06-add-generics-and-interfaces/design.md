## Context

Generic declarations parse (`FuncDecl.generics`, `TypeDecl.generics`, `TypeParam.bound`, `TypeBody::Interface`) and the compiler ignores them: `declare` and `generate` skip any function with `generics` (`src/codegen_x64.rs`), and `resolve` finds no record for `Stack<int>`. Everything downstream (signatures, records, methods, ownership, closures, the generated assembly) works on concrete `Ty` values, and methods are ordinary functions with a receiver whose `Sig.receiver` is a `Ty`.

## Goals / Non-Goals

**Goals**
- Reuse the existing compiler for every instance: an instance is just a concrete function or record, so reference counting, errors and optimizations behave exactly as for hand-written code.
- Good error messages at the generic definition and at the instantiation that fails a bound.

**Non-Goals**
- Dynamic dispatch (interface-typed values), explicit type arguments at call sites, generic enums and aliases, specialization, numeric bounds.

## Decisions

1. **Monomorphization by AST substitution.** A generic function or record is stored as a template (its `FuncDecl` or `TypeDecl` plus the module). An instance is made by cloning the declaration, replacing every use of a type parameter in `Type` nodes with the concrete `Type`, clearing `generics`, and giving it a mangled name. The cloned declaration goes through the normal `func_sig`, `gen_func` and record paths. No new code generator is needed.
2. **Instance names.** `first<int>` becomes the symbol `first__int`, `Stack<text>` the record `Stack__text` (the type's display name stays `Stack<text>` for messages). Type arguments are mangled from their `show()` text with non-identifier characters replaced, and an instance table keyed by `(template, args)` makes the same arguments reuse one instance.
3. **Instantiation queue.** `resolve` and call sites request instances while code is being generated; requests are queued and processed after the current function, repeatedly, until the queue is empty. Instances can request further instances (recursion, `Stack<Stack<int>>`); the table makes recursion terminate. A depth limit of 64 reports a runaway instantiation.
4. **Generic records.** `resolve` on `Type::Named { path, args }` with a template record checks the argument count, resolves the arguments, instantiates the record (substituting field types) and returns `Ty::Rec(instance_name)`. Methods whose receiver is `Stack<T>` are templates attached to `Stack`; instantiating `Stack<int>` also instantiates its methods (all of them, when the instance is created). Methods with type parameters of their own are rejected for now.
5. **Inference.** At a call to a generic function, parameter types are unified with the argument types left to right: a `T` binds to the argument's `Ty`, structured types (`[]T`, `Map<K, V>`, `Stack<T>`, `Func`) unify element by element, and a second, different binding is an error naming both. An integer literal argument binds `int`. Parameters that stay unbound are then resolved from the expected type by unifying the return type with it; any still unbound is the "cannot infer" error. Argument types are found by compiling each argument that mentions a type parameter into a scratch buffer that is thrown away (a snapshot of the compiler state is restored afterwards), so the real call still evaluates every argument exactly once. Lambda arguments are skipped: their parameter types come from the parameter type once the other arguments have fixed it.
6. **Checking the template.** The body of a generic function is checked once with each type parameter as an opaque `Ty::Param(name, bound)` while the output goes to a discarded buffer. Opaque types support assignment, passing to other generics, return and text interpolation (checked per instance); field access, operators and calls fail with a message naming `T`; a method call resolves against the bound's interface and type-checks arguments and result. This is what produces the errors at the generic definition. Instances are then generated without re-reporting those errors.
7. **Interfaces.** `TypeBody::Interface` is recorded as `interface_decls[name] -> Vec<MethodSig>` (resolved lazily, since signatures can mention other types). Satisfaction checks `lookup_method(Ty, name)` for each signature and compares parameter and result types with `==` after resolution. The error lists the first missing method or the first mismatch. Methods on built-in types come from receivers declared in Brasa (`func (n: int) to_text()`), which the method table must accept for non-record receivers; that lookup is generalized from `Ty::Rec` to any `Ty`.
8. **Bounds.** At instantiation, each type argument is checked against its parameter's bound before the instance is created, so the error is reported at the call that supplies the bad type. A bound on a record parameter is checked the same way when `Stack<Dog>` is resolved. Inside an instance, the bound is irrelevant: `x.to_text()` resolves to the concrete method like any call.
9. **Interfaces as types are rejected.** `resolve` returns an error for a name that is an interface when it is not in bound position, suggesting `<T: Name>`.
10. **Unsupported forms** (generic enums, aliases, async, closures, extern) fail in `declare` with a message naming the form, replacing today's silent skip.
11. **Interpreter.** The interpreter ignores type arguments and bounds, so generic functions and records already run; tests confirm and `examples/generics.br` is compared across both back ends.
12. **Modules.** Templates are looked up through the same path rules as functions and records, so `use` and qualified names (`pkg.Stack<int>`, `T: pkg.Printable`) work; an instance is emitted in the template's module so its private names resolve.

## Risks / Trade-offs

- **Inference with side effects.** Evaluating an argument twice would duplicate effects. Mitigation: infer from a side-effect-free typing pass, falling back to a scratch emit for complex expressions, and test that arguments run once.
- **Code size** grows per instance. Accepted and documented; the optimizer item can merge identical instances later.
- **Opaque `Ty::Param` leaking into codegen.** Mitigation: the checked-once pass writes to a throwaway buffer and any opaque type reaching real emission is an internal error with a clear message.
- **Method tables keyed by record** need generalizing to built-in receivers. Mitigation: do that step first and cover it with tests before generics use it.
- **Error message quality** depends on naming `T` and the interface in every failure; tests assert the wording.
