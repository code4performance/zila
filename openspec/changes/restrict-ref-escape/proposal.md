## Why

`ref` passes a record, list or map by reference, and today nothing stops the callee from letting that reference outlive the call. Inside the callee a `ref` parameter is just a pointer to the caller's value (a local, a field or a list element), so these programs compile and are use-after-free or dangling-pointer bugs at run time: returning `&r`, taking `&r` and storing it in a record or global, or capturing `r` in a lambda that is stored or handed to `spawn`. There is no borrow checker and no lifetimes, and the project does not want either. The remaining way to keep memory safe outside `unsafe` is to define `ref` as only a passing mode, a temporary borrow that cannot leave the call, and to make the compiler reject every use that could extend it.

## What Changes

- `ref` is only a parameter and receiver passing mode, not a type. A `ref` parameter, a `ref` receiver and a `ref` lambda parameter are called a "ref binding" in this change.
- **BREAKING**: outside `unsafe`, the compiler rejects any use of a ref binding that could outlive the call: storing it in a record field, list, map or global; returning it; capturing it in any lambda (no escape analysis: any lambda that mentions it); passing it to `spawn` or to an `async` function; assigning it to another variable (`let`, `var`, assignment); passing it to a by-value parameter; and taking `&` of it. Passing it on as `ref r` to another function or method is allowed, and so are reading and writing its fields and elements, calling methods on it, indexing, iterating, and using `r.len`.
- The error says that a `ref` cannot escape and tells the programmer to use `new T` (from `add-weak-refs`) for a value that must live longer.
- Scalar ref bindings (`ref v: int`) are mutable aliases of a caller variable. Reading them as a value is a copy and stays allowed; `&v` and capture are rejected like for records.
- `async` functions and `async` methods still take no `ref` parameters (the existing rule is kept and specified).
- Methods with `(ref s: T)` receivers get the same treatment: `s` is a ref binding in the body, so `return s`, `registry.add(s)` and `let t = s` in such a method are errors. The standard library and examples were audited: no current use violates the rule, so `std/`, `os/` and `examples/` need no rewrite beyond what the verification task confirms.
- `brasa.ebnf` comments, `docs/syntax.html` and `docs/native.html` describe `ref` as a passing mode with the restrictions.

## Capabilities

### New Capabilities

- `ref-parameters`: what `ref` means at a declaration and at a call site, the exact set of rejected and accepted uses, the error text, and the async restriction.

### Modified Capabilities

None. No existing spec defines `ref`; `async-execution` is unchanged (its `ref` rule lived only in code and the grammar comment and moves to the new spec).

## Impact

- `src/codegen_x64.rs`: a new check run from `gen_func` and for lambda bodies (`lambda`), plus an `is_ref` flag on `Local` set by `bind_params`; `address_of` and `func_sig` (async rule) are touched. The AST walk lives in a small new module `src/refcheck.rs`.
- `brasa.ebnf`, `docs/syntax.html`, `docs/native.html`, `docs/async.html`, `docs/index.html` wording.
- Tests: new `tests/refs.rs` (accepted and rejected programs); `tests/codegen.rs` keeps the async test.
- `std/*`, `os/*` and `examples/*` must compile unchanged; this is checked by the existing suites (`tests/stdlib.rs`, `tests/codegen.rs::examples_compile`).
