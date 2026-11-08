## 1. Checker

- [x] 1.1 Add `is_ref: bool` to `Local` in `src/codegen_x64.rs` and set it in `bind_params` from the real `by_ref` of each parameter and receiver (the tuple's third element is `true` for every parameter today and unused; pass `p.by_ref` from `sig.params`). Verify: `cargo build` and `cargo test --test codegen --test generics` stay green.
- [x] 1.2 Create `src/refcheck.rs` (declared next to the other modules) with `check_ref_escapes(names: &[(String, bool)], body: &[Stmt])` returning the offending name and line, implementing design rule 1: borrowing positions, scalar exception, scope-aware shadowing (`let`, `var`, `for`, inner parameter and lambda parameter names), lambda bodies and `spawn` operands rejected on any mention, `unsafe` blocks skipped. Verify: unit tests in the module on parsed snippets for each position.
- [x] 1.3 Call it from `gen_func` (receiver and params with `by_ref`; scalar decided from `sig`) and from `lambda` for the lambda's own `ref` parameters; report through `self.err` with the single message of design rule 6 and the line. Verify: the rejected cases in `tests/refs.rs`.
- [x] 1.4 In `address_of`, reject an `Ident` whose `Local.is_ref` is set unless `unsafe_depth > 0`, with the same message. Verify: the `&r` test.
- [x] 1.5 Keep the async rule in `func_sig` ("cannot take `ref` parameters"), with a comment pointing to the `ref-parameters` spec. Verify: `cargo test --test codegen async_functions_cannot_take_ref_parameters`.

## 2. Tests (new `tests/refs.rs`, compile-error style of `tests/codegen.rs`: `generate_asm(&parse_source(src)?)` and assert on the message)

- [x] 2.1 Rejected, one test per case, asserting "cannot escape" and "use `new T`": `return r`; `h.last = r`; `xs.add(r)`; `m[k] = r`; `[r]` and `Holder{r: r}`; global assignment; `let c = r`; `var c = r`; `c = r`; lambda mention (stored, and called at once); `spawn work(r)`; `spawn work(ref r)`; call of an `async` function with `r`; `&r`; `f(r)` to a by-value parameter; `ref` receiver `return s` and `registry.add(s)`; a lambda's own `ref` parameter returned. Verify: `cargo test --test refs`.
- [x] 2.2 Accepted: field read and write, `r.items.add`, `for v in r.items`, `r[i] = v`, `g(ref r)` through two calls, a method call on `r`, scalar `ref v: int` with `v += 10; return v`, `let n = r.name`, shadowing by an inner `let` and by a lambda parameter, and the same escapes inside `unsafe`. Verify: `cargo test --test refs`.
- [x] 2.3 Add an `async` method with a `(ref s: T)` receiver to the existing async-ref test. Verify: `cargo test --test codegen`.

## 3. Standard library, os and examples audit

- [x] 3.1 Confirm no violation in every current `ref` user: `std/fs.br`, `std/fs.linux.br`, `std/net.br`, `std/net.linux.br`, `std/sys.br`, `std/sys.linux.br`, `std/task.br`, `std/task.linux.br` (the `drop` receivers, `wake_one`, `chan_*`), `examples/types.br`, and `os/` (grep `\bref\b`; none expected). Rewrite any hit the suites reveal (use `new T` if `add-weak-refs` has landed, otherwise an index or a field copy). Verify: `cargo test --test stdlib --test codegen --test linux --test defer --test optimizer` (leave `tests/async.rs` out of routine runs) and `zila build examples/types.br`.

## 4. Documentation

- [x] 4.1 `zila.ebnf`: update the comments on `lambda_param`, `arg`, the receiver and the "Values" notes ("ref = temporary borrow of a value that lives elsewhere; it cannot be stored, returned, captured or sent to a task"); productions unchanged. Verify: `cargo test --test parse`.
- [x] 4.2 `docs/syntax.html` (the `ref` paragraphs), `docs/native.html`, `docs/async.html`, `docs/index.html`: describe `ref` as a passing mode with the escape rule and the `new T` remedy. Verify: `cargo test --test docs`.
- [x] 4.3 Check README and GOVERNANCE for `ref` mentions. Verify: `grep -n "\bref\b" README.md GOVERNANCE.md` and update if any.

## Notes

- Implemented as designed. Defaults taken: scalar = `plain_scalar` (int, bool, float, pointer); `defer` operands, `match` scrutinees and text interpolations count as value positions; `unsafe` and inline `asm` blocks are skipped; assignment to a ref name itself (`r = x`) is allowed.
- Lambdas have no `->`, so tests use plain `func()` lambdas.
- Audit found one violation: `release(conn)` in `std/tls.br` and `std/tls.linux.br` (called from the `ref` receiver `drop`). Fixed by making `release` take `ref conn` and passing `ref conn` at every call.
- Not run (needs permission / network): `tests/linux.rs`, `tests/os.rs`, `tests/tls.rs`, `tests/dns.rs`. The `.linux.br` std files were audited by reading only.
- `zila build examples/types.br` fails with "type 'Customer' is not supported by the native backend" (unrelated to this change; no escape error).
- Test results: refs 17/17, refcheck unit 2/2; remaining failures equal the known pre-existing set.
