## Context

All calls to Brasa functions go through `call_sig(sig, receiver, args)` in `codegen_x64.rs`: it binds the arguments to the parameters, evaluates each into a temporary slot (`temps`), loads the first four into `rcx, rdx, r8, r9` and emits `call sym`, then frees the temporaries. Each function is compiled into its own `FnState` (scopes, slots, labels, the register plan). Locals live at absolute offsets in the frame, so code for another function's expression can be emitted inside the caller as long as names resolve in the right places. A function's module is `sig.module`; default-parameter expressions are already compiled in the callee's module by switching `self.cur_mod`.

## Goals / Non-Goals

**Goals**
- Remove the call overhead of tiny functions in hot code, with a simple, auditable rule for what is inlined.
- Keep every semantic detail: single evaluation of arguments in order, name resolution, run-time errors, `--no-opt` as the reference.

**Non-Goals**
- Inlining functions with statements (`if`/`while` bodies, several `return`s), methods, generics, text/list/record parameters or results, recursion, `extern`, `async`. Each is a possible later step; the rule below is deliberately narrow.
- Propagating constant arguments into the body to fold it further (a later step on top of this one).
- Inlining calls made through function values (they stay calls).

## Decisions

1. **What is inlinable.** When signatures are collected (and `opt` is on) each non-generic, non-method, non-extern, non-async, non-naked function with at most 4 parameters (no defaults, no `ref`, not variadic) whose body is exactly one `return e` is checked: all parameter types and the result are integers, `bool`, floats or raw pointers; `e` has at most 12 nodes (every `Expr` counts one) and contains no `Lambda`, `Match`, `If` expression, `Propagate`, `Await`, text interpolation, record/list/map literal, `new`, and no call to itself. Its `Expr` and module are stored in `inline_bodies: HashMap<sym, (Expr, module)>`. A body that calls other functions is allowed (they may inline in turn); a depth counter (at most 4 nested inlines) bounds mutual recursion, and past it the call is emitted as a call.
2. **Where it plugs in.** In `call_sig`, after the checks and argument binding but before the register moves: if `opt` is on and `sig.sym` is in `inline_bodies`, then instead of the call sequence the compiler (a) evaluates the arguments as today into temporaries (or aliases them, see 4), (b) swaps in a fresh scope list containing one scope whose variables are the parameters, (c) switches `cur_mod` to `sig.module`, (d) compiles the body expression with `value(e, Some(&sig.ret))`, checks it against `sig.ret` and normalizes it like a `return` does, (e) restores `cur_mod` and the caller's scopes, and frees the temporaries. The result is in `rax` exactly as after a call.
3. **Isolation of names.** Swapping `fs.scopes` for a one-scope list guarantees the body cannot see the caller's locals; `cur_mod` makes globals, functions and imports resolve in the callee's module. The register plan (`fs.regs`) is keyed by names of the caller's variables and applies only in `declare_local`/`bind_params`, which the inlined body never calls, so it cannot capture a name of the callee.
4. **Argument aliasing.** A parameter is immutable inside the body and the callee cannot assign the caller's variables (no by-reference parameters are allowed, closures copy), so an argument that is a literal or a plain local variable (in a register or a slot, not captured) needs no temporary: the parameter is bound to the same `Local` (same register or slot) or to a literal that `simple_rhs` loads as an immediate. Anything else is evaluated into a temporary slot as today. Evaluation order is preserved because only side-effect-free arguments are aliased, and the others are evaluated left to right as before. An aliased variable cannot change while the body runs (the body has no statements and cannot assign it).
5. **Run-time errors.** Failures inside the body use the same helper calls as the out-of-line function, so messages and exit codes are the same. Expressions compiled for the body leave temporaries registered in the caller's statement exactly like any other expression of the caller, so the caller's `flush_temps` frees them.
6. **The function stays.** Inlining never removes a function; it is still compiled, and used by calls that were not inlined or by function values. A function used only by inlined calls costs a little code size, which is accepted.
7. **`--no-opt`.** The table is not built and the branch is not taken when `opt` is off, so `--no-opt` output is unchanged.
8. **Code size.** The 12-node limit keeps a body about the size of the call sequence it replaces. The limit and the nesting depth are constants at the top of the module.
9. **Testing.** The existing differential helper compiles every program both ways, so every suite already exercises inlining; new tests target the rules above (order, single evaluation, shadowing, modules, errors), assembly shape (`call` present or absent, and with `--no-opt`), generated random programs with helper functions and colliding names, and the benchmark.
10. **Measurement.** A benchmark of 100 million iterations calling three small helpers is added to `benchmark_report` with the 25% assertion; the existing assertions stay.

## Risks / Trade-offs

- **Name capture bugs** (the caller's local read by the body, or the reverse) are the main risk. Mitigation: the scope swap, tests with deliberately colliding names, and random programs with colliding names.
- **Aliasing** is the rule most worth a second look in review; the evaluation-order test covers an argument that is a variable next to an argument whose side effect would matter if the order changed.
- **Typing differences**: the body is compiled with the expected type of each parameter bound to the argument's already-normalized value, so integer and float widths behave like the out-of-line version. Tests cover `i8`, `u32` and `f32` helpers.
- **Gains are bounded** by how many hot calls are tiny; larger functions are not helped.

## Measurements

Debug build of the compiler, best of three runs, `--no-opt` -> optimized (from `benchmark_report`):

| Benchmark | Instructions of the hot function | Run time |
|---|---|---|
| fib(38) | 53 -> 30 | 1.04 s -> 0.37 s |
| loop (100 M iterations) | 53 -> 39 | 245 ms -> 76 ms |
| join | 90 -> 75 | 52 ms -> 62 ms (noise level, unchanged by this change) |
| helpers (100 M iterations, three small functions) | 72 -> 64 | 660 ms -> 246 ms (about 2.7x) |

## Implementation notes

- Literal arguments are passed as an `imm` value of the parameter's `Local` (read by `load_local` as `mov reg, imm`); a local variable argument is aliased only when its type equals the parameter's and every later argument only reads variables (no call that could change it).
- Heap values that calls inside an inlined body leave to be released are handed to the caller's innermost scope when the temporary scope is dropped.
- The caller's generic bindings are cleared while the body is compiled.
