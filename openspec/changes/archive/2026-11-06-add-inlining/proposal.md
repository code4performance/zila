## Why

Small helper functions (`square(x)`, `is_even(n)`, `clamp(v, lo, hi)`, record getters written as functions) are everywhere in Zila Lang code, and each call pays for an argument shuffle, a `call`, a prologue that builds a stack frame, an epilogue and a `ret`, which is often more work than the one expression the function computes. The optimizer and register allocation made the rest of a hot loop cheap, so the calls now dominate. Inlining them is the next roadmap step.

## What Changes

- With optimization on, a call to a small function whose body is a single `return <expression>` is compiled in place: the arguments are evaluated once, in order, as before, and the function's expression is compiled at the call site with the parameters bound to those values. The function itself is still compiled, so function values and calls from other places keep working.
- A function is inlined only when it is small (a handful of operations), takes and returns only plain numbers, booleans or raw pointers, has no receiver, no generics, no `asm`, `defer`, `try`, `await`, lambda or `match`, is not `extern`, `async` or `naked`, and does not call itself. Anything else is called as before.
- Arguments that are literals or plain local variables are used directly instead of being copied into temporaries, which is what makes a call of a tiny function as cheap as writing its body.
- `--no-opt` never inlines, so it stays the reference for the differential tests.
- A benchmark with small helper calls in a hot loop must run at least 25% faster than with `--no-opt`, and the existing benchmarks must not get slower.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `optimizer`: adds the requirements for inlining small functions (what is inlined, what is never inlined, behavior preserved, measured speedup).

## Impact

- `src/codegen_x64.rs`: a table of inlinable bodies built when function signatures are collected, a branch in `call_sig` that binds parameters and compiles the body in an isolated scope, and the argument-aliasing shortcut.
- `tests/optimizer.rs`: differential tests for inlined calls (evaluation order, side effects in arguments, shadowing, module-level names, runtime errors inside inlined bodies), assembly tests (no `call` left, calls kept for ineligible functions) and a benchmark assertion.
- `docs/native.html` and the README: describe inlining and update the roadmap.
