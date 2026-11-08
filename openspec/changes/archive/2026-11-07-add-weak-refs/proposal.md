## Why

Zila Lang frees heap values by reference counting, and a reference cycle is never freed: the README says "avoid building them", and `docs/native.html` repeats it. Trees with parent links, observers, doubly linked lists and graphs all need a cycle, so today they either leak or are written around with indexes. The leak is invisible until a long-running program (a server, the kernel in `os/`) grows.

Records are value types: assigning one copies it, so a child cannot hold "the" parent record, only a copy. `ref` shares only for the duration of a call, and `#[shared]` is an internal attribute for library records. There is no way for user code to say "this object is shared, and its lifetime is its reference count". `new` is in the grammar (`new_expr = 'new' type`) but the native compiler rejects it (`'new' is not supported by the native backend yet`) and the interpreter evaluates it to `nil`, so the keyword is free to carry exactly that meaning.

## What Changes

- `new Node{ value: 1 }` becomes the one explicit way to make a shared, reference-counted (non-atomic) heap object. Its type is the new generic type `Rc<Node>`. Records themselves stay value types.
- `Weak<T>` is a new generic type: `weak(x)` makes one from an `Rc<T>`, `w.upgrade()` returns `Rc<T>?` (`nil` once the last strong reference is gone). A weak reference does not keep the object alive.
- A field can hold a shared parent without a leak: `parent: Weak<Node>`. A parent/child tree built this way is freed completely, which `live_objects()` shows.
- `drop` runs when the strong count reaches zero, before the fields are released, and `upgrade()` called from inside `drop` returns `nil`.
- Closures capture an `Rc<T>` strongly (by value, as all captures) and a `Weak<T>` weakly.
- Documentation (`docs/native.html`, `docs/syntax.html`), an example and the README are updated; the "cycles leak" advice becomes "break cycles with `Weak`".
- **BREAKING**: `new` changes from "in the grammar, unsupported" to a typed expression that requires a record literal (`new T` with no literal is a compile error). No existing program can use it, since the native compiler rejected it.

## Capabilities

### New Capabilities
- `weak-references`: shared reference-counted objects (`new`, `Rc<T>`), weak references (`Weak<T>`, `weak`, `upgrade`), their lifetime, `drop` interaction, assignment, comparison and closure capture rules.

### Modified Capabilities
<!-- None: no existing spec covers `new`, reference counting or cycles. `defer`, `generics` and `maps` are used as they are. -->

## Impact

- **Compiler** (`src/codegen_x64.rs`): new types `Ty::Rc(Box<Ty>)` and `Ty::Weak(Box<Ty>)` (`resolve`, `nullable`, `show`, copy/drop helpers, field access and method calls through an `Rc`), `ExprKind::New` replacing the error at the `ExprKind::New(_)` arm, the builtin `weak` next to `live_objects`, the `upgrade` method, and the runtime support for the weak handle and the per-record drop wrapper.
- **Parser** (`src/parser.rs`, `zila.ebnf`): `new` followed by a record literal.
- **Formatter / language server** (`src/fmt.rs`, `src/lsp.rs`): only where they list types or builtins.
- **Documentation**: `docs/native.html` (memory section, not-yet-supported list), `docs/syntax.html` (`new` paragraph), new example `examples/tree.br` with a section on `docs/examples.html`, `README.md`.
- **Standard library, `os/`, existing examples and tests**: audited for hand-made parent links; none is expected to change, and the audit result is recorded in the tasks.
- **Assumptions**:
  - The type is spelled `Rc<T>`. The wording "`new T` is the shared type" needs a spelled name for fields and signatures; `Rc` is the familiar name for a non-atomic counted pointer. Easy to rename before implementation.
  - `Weak<T>` accepts `nil` (it is nullable, like text and lists), so a root node can write `parent: nil`; `upgrade()` on `nil` returns `nil`.
  - The interpreter is not taught `new` (see `remove-interpreter` in design.md).
