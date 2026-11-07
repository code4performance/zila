## Context

How `ref` works today (`src/codegen_x64.rs`):

- Parameters and receivers carry `by_ref` (`Param`, `Receiver`, `Arg`, `ParamInfo`). At a call, a non-ref argument goes through `self.value` (records are deep-copied by `B_copy_*`), while a ref argument goes through `self.expr`, so the callee receives the caller's pointer (the argument loop that tests `p.by_ref`).
- `bind_params` stores every parameter in a stack slot and registers a `Local`. The `Local` has no notion of being a ref, so nothing downstream can tell `r` (a borrowed pointer) from an owned local.
- Escape routes that compile today: `&r` (`address_of` loads the record pointer and returns `Ptr`), `return r` and storing `r` where the value is not copied, capture in a lambda (`lambda` captures by value, but `#[shared]` records, lists and maps are shared, and `spawn` is compiled as a lambda), and `spawn work(r)`.
- Async: `func_sig` already rejects `ref` parameters and receivers on `async` functions ("cannot take `ref` parameters").
- All current `ref` uses in `std/` (`drop` receivers in `fs.br`, `net.br`, `sys.br`; the `Chan` helpers `wake_one`, `chan_send`, `chan_recv`, `chan_close` in `task.br`; and the `.linux.br` twins) and in `examples/types.br` only read or write fields, call methods or pass `ref ch` down. None escapes, so the rule is a pure restriction with no library rewrite expected. `tests/optimizer.rs` and `tests/defer.rs` use scalar and record `ref` the same benign way.

## Decisions

1. **Syntactic rule, no flow analysis.** A ref binding is a name introduced by a `ref` parameter, receiver or lambda parameter. The check walks the function body with the set of live ref names (a `let`, `var`, `for` binding or inner parameter with the same name removes it for its scope). An occurrence of the name is allowed only in these "borrowing positions":
   - the base of a field access, index, method call or `.len` (`r.x`, `r[i]`, `r.m()`, `r.x.y = 1`, `r[i] = v`);
   - the iterated expression of `for x in r`;
   - the operand of a repeated `ref` in a call argument (`f(ref r)`, `g.m(ref r)`);
   - for scalar ref bindings only, any value position (a read is a copy) and the target of an assignment (`v += 10`), except operand of `&` and capture.
   Every other occurrence is an escape: right side of `let`, `var` or assignment, element of a list, map or record literal, argument without `ref`, `return`, operand of `&`, operand of `spawn`, argument of an async call, any mention inside a lambda.
   Rationale: every way to copy the pointer is a bare occurrence, so the rule is complete, needs no types beyond scalar vs not, and is easy to explain. The cost is that `let c = r` (which would copy a record and be safe) is rejected too; the owner chose this over a more precise rule. Reading a field (`let n = r.name`) copies the field value and is fine.
2. **Lambdas: any mention is an error.** A lambda that mentions a ref binding is rejected even if it is called immediately. `spawn call(...)` is compiled as a lambda, so `spawn work(ref r)` and `spawn work(r)` are rejected by the same rule with the same message.
3. **Async calls.** Passing a ref binding to an `async` function is covered by rule 1 (a bare `r` is an escape, and `ref r` cannot match a parameter because async functions take none). `async` functions and methods keep rejecting `ref` parameters at declaration; this stays in `func_sig` with its current message.
4. **Inside `unsafe` the rule is off.** The memory-safety promise is for code outside `unsafe`, so an `unsafe` block may take `&r` and store it. The check treats `unsafe_depth > 0` (already tracked in `FnState`) as exempt.
5. **Where to implement.** The walk is pure over the AST (`src/refcheck.rs`); the caller passes the ref names and whether each is scalar (decided from the resolved `sig` in `gen_func`). It is called from `gen_func` and from `lambda` for a lambda's own `ref` parameters. `Local` gets `is_ref: bool` so `address_of` can repeat the check as defense in depth. The interpreter is being removed (`remove-interpreter`) and is not touched.
6. **Message.** One text for every escape: "`ref` parameter 'r' cannot escape: it is only a temporary borrow and cannot be stored, returned, captured or sent to a task; use `new T` for a value that must live longer", with the source line. Tests assert on the fragments "cannot escape" and "use `new T`".
7. **Methods.** A `(ref s: T)` receiver makes `s` a ref binding. `s.field = v`, `s.items.add(x)`, `s.other()` and `helper(ref s)` are accepted; `return s`, `registry.add(s)`, `let t = s` and `&s` are rejected. A non-ref receiver `(s: T)` is a copy and unrestricted.
8. **Grammar.** `brasa.ebnf` keeps `[ 'ref' ]` in `param`, `receiver`, `lambda_param` and `arg`; only the comments change.

## Depends on

- `add-weak-refs`: the error message points to `new T`, the heap value that can outlive a call. The check and its tests do not need `new` to exist; the message text is fixed now, and the docs name `new T` as the remedy (as planned until that change lands).
- `remove-async-coloring`: if `async` stops being a function modifier, the "async functions take no ref parameters" rule and its test are replaced by whatever that change defines for tasks. The `spawn` rule here stays.
- `add-threads-and-shared`: spawn and thread starts must keep rejecting ref bindings; that change reuses the lambda rule.
- `remove-interpreter`: independent; this change only edits the compiler.
- `add-direct-pe-output`: independent; the tests use `generate_asm` (compile-error style of `tests/codegen.rs`) and need no linker.
- `add-copy-on-write`: if plain assignment of records becomes cheap and safe, revisit rule 1's rejection of `let c = r`.

## Risks

- Some valid-looking helper code (`let c = r`, `f(r)` to a by-value parameter) needs rewriting as a field read or an explicit `ref`. The message names the remedy; the repo has no such use today.
- Shadowing must be handled correctly, or a legal program is rejected or an escape is missed. Tests cover an inner `let` and a lambda parameter that shadow.
- Capture is rejected even when harmless (an immediately called lambda). Accepted by decision.
