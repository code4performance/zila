## Context

See proposal.md for the motivation. Facts about the compiler today (`src/codegen_x64.rs`):

- Every heap object has a 16-byte header `[refcount][drop fn]` in front of its pointer. `rt_retain` adds one, `rt_release` subtracts one and, at zero, calls the drop function (if any) and then `rt_free_obj`, which decrements `rt_live`. `live_objects()` reads `rt_live`.
- Records are heap objects too, but have value semantics: a record read from a variable, field or list is deep-copied (`B_copy_*`) when stored elsewhere. The internal attribute `#[shared]` (set `shared_recs`) switches the copy off; the standard library uses it for files, sockets and futures.
- `ExprKind::New(Type)` exists in `src/ast.rs` and `src/parser.rs`, but codegen answers `'new' is not supported by the native backend yet` and `src/interp.rs` returns `Value::Nil`.
- `Optional` is allowed only for types where `Ty::nullable()` is true.
- Closures are `[code][env]` and capture by value.

## Goals / Non-Goals

**Goals:**
- One explicit, user-visible shared object type, with a weak counterpart, so cycles can be broken.
- No cost for programs that do not use them; records and every existing type keep their behavior.
- A `live_objects()`-based test proving a weak parent/child tree frees everything.

**Non-Goals:**
- Atomic counts, sharing between threads, a `Shared`/`Sync` rule: that is `add-threads-and-shared`. `Rc<T>` is non-atomic and tasks on the one scheduler are the only users.
- Cycle detection or a collector. A cycle of strong references still leaks and the docs say so.
- Copy-on-write or copying an object out of an `Rc` into a value (`add-copy-on-write`), and restrictions on references escaping (`restrict-ref-escape`).
- Replacing `#[shared]` for library records.

## Decisions

**1. `new T{...}` produces `Rc<T>`; the record type stays a value.** `new` requires a record literal (the grammar becomes `new_expr = 'new' record_lit`), so there is no undefined "zero record". The object is the ordinary record allocation, flagged as shared in the type (`Ty::Rc(Box<Ty>)` wrapping `Ty::Rec`), not at run time. The value is a pointer to the object, so copying an `Rc` is `rt_retain` and the existing ownership rules (owned results, scopes release locals, arguments handed over) apply unchanged. `Rc<T>` is treated as a non-record type, so the places that deep-copy `Ty::Rec` (assignment, capture, list iteration) skip it for free. Alternative: make `new T` yield plain `T` with a hidden flag. Rejected: a value that is sometimes shared and sometimes copied depending on how it was made makes assignment unpredictable, which is the problem the decision removes.

**2. Field access and method calls go through the `Rc`.** `n.value`, `n.value = x` and `n.method()` resolve against `T` and operate on the shared object; a `ref` receiver gets the object pointer itself. There is no implicit conversion `Rc<T>` to `T`: passing it where a `T` is expected is a type error (copying out is `add-copy-on-write`'s concern). Alternative: explicit `*n`. Rejected: auto-deref matches how `ref` receivers already read.

**3. The weak handle is a separate counted "cell".** The strong object cannot be both freed at strong zero and kept for weak holders, and `rt_release` frees the block right after the drop function. So the object does not stay; a small cell does:
- A shared object gets one extra word at its start, `cell`, `0` until the first `weak(x)`. `weak(x)` allocates the cell lazily: an ordinary heap object `[hdr: count][drop: none][target]`, with `target` pointing to the object; the object owns one reference on the cell, and each `Weak<T>` value owns another.
- `Weak<T>` is the cell pointer itself, so copying one is `rt_retain` and releasing one is `rt_release`; no new release path is needed and the cell is freed when its last holder goes. `nil` is the null pointer, already handled by retain/release.
- `upgrade()` reads `target`: if non-zero it retains the object and returns it, otherwise `nil`.
- The per-type drop wrapper for a shared record (generated like `B_drop_*`) runs in this order: if `cell != 0`, store `0` in `target` and release the cell; run the user's `drop`; release heap fields. So `drop` never sees a resurrectable object, and the object is freed at strong zero whatever the weak count is.
The cell is a heap object counted by `rt_live`, so `live_objects()` for a target does not include weak references to it except for that one small cell while a `Weak` lives; it is freed with the last `Weak` and the target, which the spec states and the tests check. Alternative: keep the whole object alive until the weak count reaches zero (Rust's approach) with a second counter in the header. Rejected: it changes `rt_release` for every type and delays `drop`'d memory; the cell touches only shared records.

**4. Comparison is identity.** `==` between two `Rc<T>` compares pointers. Between two `Weak<T>` it compares cell pointers, which is the same as comparing targets because there is one cell per object. `nil` comparison is the existing nullable path. Mixed `Rc`/`Weak` or against `T` is a compile error, because a silent upgrade would hide the liveness question.

**5. Closures.** Capture is by value and copies the variable's value: an `Rc` captured is retained (strong), a `Weak` captured is a retained cell (weak). No closure change is needed beyond the new types being known to the capture and env-drop code (`Ty::Rc` and `Ty::Weak` count as heap types). The documented idiom is `let w = weak(node)` before the closure and `w.upgrade()` inside it.

**6. Generic types in the compiler, no new syntax in the type grammar.** `Rc<T>` and `Weak<T>` are resolved in `resolve` like `Map<K, V>` and `Future<T>` (built-in generic names), so `Weak<Node>` works as a field, parameter and return type, and in generic bodies. `weak` is a builtin function next to `live_objects`; `upgrade` is a builtin method like `len`. The names become reserved type names; a user type named `Rc` or `Weak` is a "defined twice" error, which the tasks verify.

**7. Interpreter and other back ends.** `--run` is not extended; it keeps evaluating `new` to `nil` until `remove-interpreter` deletes it. The bare-metal and Linux targets share the code generator and the runtime text, so they get the feature with no extra work; tests run on whichever targets the existing suites run on.

## Risks / Trade-offs

- [A strong cycle still leaks] -> The docs and README state it plainly and show `Weak` as the fix; a leak test documents the remaining behavior.
- [One extra word per shared object, one cell per weakly-referenced object] -> Only `new` objects pay; plain records are unchanged. The cell is allocated only on first `weak(x)`.
- [`Rc<T>` name and `nil`able `Weak<T>` are choices a reviewer may want different] -> Both recorded as assumptions in proposal.md; renaming is local to `resolve` and the docs.
- [`drop` clearing the cell first means a `drop` cannot hand out a new strong reference to itself] -> That is the point; the spec makes it a stated rule.

## Migration Plan

No migration for programs: `new` could not be used before. Pre-1.0 policy (GOVERNANCE.md) allows the one breaking change (`new` requires a record literal). The change rewrites docs and README in the same change and audits `std/`, `os/` and `examples/` for any hand-rolled parent link or cycle workaround (index tables, copies) that should now use `Weak`; if none is found that is recorded, not invented.

Depends on:
- `add-direct-pe-output`: no functional dependency; tests that run native executables use whatever build path exists when this is implemented.
- `remove-interpreter`: decides whether the interpreter's `New` arm is deleted or never touched; this change does not touch it.
- `add-visibility-pub`: if it lands first, `Rc`/`Weak` stay built-in and need no visibility; no ordering required.
- `add-copy-on-write`: owns copying the contents out of an `Rc<T>`; until it lands there is deliberately no way to do so.
- `add-threads-and-shared`: owns atomic counts and thread-safe sharing; `Rc<T>` here MUST NOT be sent between threads, and that change decides how that is enforced.
- `restrict-ref-escape`: interacts with `ref` receivers on an `Rc`; this change only passes the shared object pointer.
- `add-no-alloc-and-allocator`: `new` is an allocating operation and must be rejected in a `#[no_alloc]` function (the README already lists `new`); `weak(x)` allocates the cell on first use and counts too.
- `add-derived-eq-ord-hash`: derived equality on a record with an `Rc<T>` field would compare identity by the rule in decision 4.

## Open Questions

- Spelling of the type (`Rc<T>` assumed). Decide before implementation.
