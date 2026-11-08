## 1. Native compiler

- [x] 1.1 Reject a `defer` whose operand is not a call, with a message saying `defer` needs a call; verify with a compile-error test
- [x] 1.2 Create the deferred-closure list at function entry for functions and lambdas that contain a `defer`, push a closure for each executed `defer` (records shared, failable results discarded); verify a program that defers a print runs it at the end of the function
- [x] 1.3 Run the deferred closures last-in-first-out at every exit (`return`, falling off the end, failed `try`) while preserving the return value; verify order, early return, `try` failure and that the returned value (including `T or error`) is intact
- [x] 1.4 Verify capture rules, loops and conditions, `break`/`continue`, lambdas and async functions with tests (a number changed after `defer`, a record changed after `defer`, a `defer` in a loop, a `defer` not reached)

## 2. Interpreter

- [x] 2.1 Run deferred calls at function exit, last in first out, in the interpreter, replacing the current silent skip; verify with `--run` on a program with two defers

## 3. Hardening

- [x] 3.1 Add a leak test: a function with a `defer` called 5000 times leaves the live-object count unchanged; verify with `live_objects()`
- [x] 3.2 Verify a deferred call that can fail compiles and does not change the function's result, and that a deferred call to a method of a record with `drop` still releases everything

## 4. Documentation and examples

- [x] 4.1 Add `examples/defer.br` and a section for it on `docs/examples.html` (generated with `zila-parse --docs`); verify the example builds and prints the documented output
- [x] 4.2 Document `defer` on `docs/syntax.html` (function-scoped, last in first out, captured when it runs, where it does not run), remove it from the not-yet-supported lists in `docs/syntax.html` and `docs/native.html`, and update the status and roadmap in `README.md`; verify `cargo test` (docs tests) passes and every code sample compiles
