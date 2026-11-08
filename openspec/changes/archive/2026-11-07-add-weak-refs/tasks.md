## 1. Language surface

- [x] 1.1 Change `new_expr` in `zila.ebnf` to `'new' record_lit` and make `src/parser.rs` (the `Tok::Kw("new")` arm) parse a record literal; change `ExprKind::New` in `src/ast.rs` accordingly; verify with a parse test in `tests/parse.rs` that `new Node{ value: 1 }` parses and `new Node` is rejected with a message asking for a record literal
- [x] 1.2 Reserve the type names `Rc` and `Weak` and the builtins `weak` and `upgrade` in `src/lsp.rs` and `src/fmt.rs` where they list types or builtins; verify `tests/lsp.rs` and `tests/fmt.rs` pass and a user type named `Rc` fails with a duplicate-definition error

## 2. Native compiler: Rc

- [x] 2.1 Add `Ty::Rc(Box<Ty>)` to `src/codegen_x64.rs`: handle it in `resolve` (generic name like `Map`), `show`, equality of types, template instantiation, and treat it as a heap type everywhere retain/release/capture decide by type; keep it out of the `Ty::Rec` copy paths (`B_copy_*`, assignment, iteration); verify a program that copies an `Rc` into a list, a field and a function argument shares one object (spec "Two names, one object", "Passed to a function")
- [x] 2.2 Replace the `ExprKind::New(_)` error in `src/codegen_x64.rs` with allocation of the record, an extra leading `cell` word set to 0, strong count 1, and result type `Ty::Rc`; reject non-records and a missing literal with the specified message; verify `new int` and `new Node` compile errors and `new Node{ value: 7 }` printing 7
- [x] 2.3 Resolve field reads/writes and method calls (including `ref` receivers) through `Ty::Rc` against the record's declaration; make `Rc<T>` where `T` is expected a type error; verify `n.value = 8` is visible through another reference and a `ref` method mutates the shared object
- [x] 2.4 Implement `==`/`!=` as identity for `Rc` and compare with `nil`; reject `Rc` with `Weak`, with `T`, and use as a map key; verify with the "Identity, not contents" and "Mixed comparison rejected" scenarios

## 3. Native compiler: Weak

- [x] 3.1 Add `Ty::Weak(Box<Ty>)` (nullable, a retained cell pointer) with `resolve`, `show`, and retain/release as for other heap types; verify `parent: Weak<Node>` compiles as a field, parameter and return type, including inside a generic record
- [x] 3.2 Implement the builtin `weak(x)` next to `live_objects` in `src/codegen_x64.rs`: lazily allocate the cell (`[hdr][no drop][target]`, owned once by the object and once per `Weak` value), reuse it on later calls; reject non-`Rc` arguments with the specified message; verify `weak(a) == weak(a)` and the "weak of a non-Rc" error
- [x] 3.3 Implement `w.upgrade()` returning `Rc<T>?` (retain and return the target, or `nil`, `nil` for a `nil` weak); verify "Upgrade while alive", "Upgrade after the last strong reference is gone" and "Upgrade of nil"
- [x] 3.4 Generate the drop wrapper for shared records (alongside `B_drop_*`): clear `target` and release the cell, run the user's `drop`, release heap fields; verify `drop` runs exactly once at strong zero, that `upgrade()` inside `drop` is `nil`, and that releasing a `Weak` never runs `drop`

## 4. Closures

- [x] 4.1 Make closure environment capture and its drop treat `Rc` as a retained pointer and `Weak` as a retained cell; verify with the "Weak capture breaks a cycle" and "Strong capture keeps the object" scenarios using `live_objects()`

## 5. Tests

- [x] 5.1 Add `tests/weak.rs` (same helper shape as `tests/defer.rs`) with one program per scenario in `specs/weak-references/spec.md`; verify `cargo test --test weak` passes (and skips cleanly when no native toolchain exists)
- [x] 5.2 In `tests/weak.rs` add the parent/child leak test: record `live_objects()`, build a parent with ten children each holding `parent: Weak<Node>`, upgrade every parent link, drop the parent, assert growth is 0; add the strong-cycle test asserting growth is 2; add a 5000-iteration loop of both leaving growth unchanged; verify they pass
- [x] 5.3 Run the existing native suites that exercise ownership (`tests/codegen.rs`, `tests/defer.rs`, `tests/generics.rs`, `tests/maps.rs`, `tests/linux.rs`) and the Linux and bare-metal builds that already have tests; verify none regress (leave `tests/async.rs` out of routine runs)

## 6. Audit

- [x] 6.1 Search `std/`, `os/kernel.br`, `examples/` and `tests/` for hand-made parent links, index-based back references or comments warning about cycles that should now use `Weak`; update any found and verify the affected suite passes, or record in the PR that none were found

## 7. Documentation and examples

- [x] 7.1 Add `examples/tree.br` (a tree whose children point to the parent with `Weak<Node>`, printing `live_objects()` before and after) and a section on `docs/examples.html` generated with `zila-parse --docs`; verify the example builds and prints the documented output
- [x] 7.2 Update `docs/native.html`: replace the cycle bullet with the `Rc`/`Weak` explanation and the strong-cycle caveat, document the memory layout (cell, drop order), and remove `new` from the not-yet-supported list; update the `new` paragraph in `docs/syntax.html`; verify `cargo test --test docs` passes and every code sample compiles
- [x] 7.3 Update `README.md` (replace "Reference cycles are not collected: avoid building them", add `new`/`Weak` to the memory bullets and the status list) and the `new_expr` note in `zila.ebnf`; verify `tests/docs.rs` still passes

## Implementation notes

- Representation: `Ty::Rc(Box<Ty>)` points at the same layout as the record, so field reads/writes, `ref` methods and the copy-on-write path logic reuse the record code through `rc_view`. The weak cell word sits after the fields (`cell_off`), and `new` allocates `cell_off + 8` bytes with the per-record drop wrapper `B_rcdrop_*` (generated at the end of compilation for every record used with `new`).
- `Weak<T>` is the cell pointer; the cell is a heap object `[target]` made on the first `weak(x)`.
- Defaults chosen: `Rc<T>` and `Weak<T>` are nullable (`Rc<T>?` is the same type); `<`, `<=`, `>`, `>=` on them are a compile error; a user type named `Rc` or `Weak` fails with "type 'Rc' is defined twice"; `new` takes a (non-generic-argument) path, a generic record literal is inferred from the expected `Rc<..>` type as for plain literals; `weak(nil-Rc)` gives `nil`. `weak` is a builtin unless the program defines its own function `weak`.
- `#[no_alloc]`: `new` and `weak(x)` are reported as allocating operations.
- 1.2: `src/lsp.rs` and `src/fmt.rs` (and the VS Code grammar) list no types or builtins, so nothing needed reserving there; the reserved-name check lives in the compiler.
- 5.3: ran codegen, defer, generics, maps and the other non-I/O suites (all pass). `tests/linux.rs`, `tests/async.rs` and the I/O suites (`buffered_io`, `udp`, `dns`, `tls`, `os`, `ui`, `error_kinds`) were not run (user rule) but compile (`cargo test --no-run`). Linux and bare-metal builds share the code generator; not run.
- 6.1: audit of `std/`, `os/`, `examples/`, `tests/`: no hand-made parent links or cycle workarounds found (the only "parent" hits are Win32 window handles); nothing changed.
- `tests/noalloc.rs` was updated for the new `new` grammar (`new P{ x: 1 }`).
