## Context

Native code generation already has what `defer` needs: closures that capture variables, scope clean-up that runs on `return`, `break` and `try`, and one place per function where all exits are emitted. `defer` is parsed (`StmtKind::Defer`) and the grammar says `defer_stmt = 'defer' expr`. See proposal.md for motivation and specs/defer/spec.md for required behavior.

## Goals / Non-Goals

**Goals:**
- Function-scoped, last-in-first-out `defer` that works with loops, conditions, `try`, lambdas and async functions.
- No leaks and no change to returned values.
- The interpreter behaves the same way.

**Non-Goals:**
- Running deferred calls on panics or program exit (there is no unwinding).
- Block-scoped defer, or assigning to captured variables from a deferred call.

## Decisions

**1. A list of closures per function, built at run time.** A function that contains a `defer` creates, at entry, a list for deferred closures (flagged as holding heap objects, so releasing it releases the closures). Each executed `defer` builds a closure for its call and pushes it. Every exit takes the closures off the end of the list one by one and calls them. Alternatives: duplicate the deferred code at each exit (cannot handle `defer` in loops or conditions without extra flags), or a compile-time stack (the number of executed defers is only known at run time). The list is the one structure that handles all cases with the same code.

**2. Reuse the closure machinery, with two switches.** The deferred call is compiled as a no-argument closure by the existing lambda code, so it captures variables by value when `defer` executes. Two small options apply to it: records are captured by sharing instead of copying (so `defer c.report()` sees later changes, and a `ref` receiver works), and a `T or error` result of the statement is discarded instead of rejected.

**3. Exits run the list before releasing locals.** At every exit (a `return`, falling off the end, a failed `try`) the compiler saves the value being returned, runs the deferred closures, restores the value, then releases the locals as it does today. Running first means a deferred call can still use things its function created.

**4. Detecting functions with a `defer`.** A function (or lambda) gets its list only if its body contains a `defer` outside nested lambdas, found by a walk of the body. Functions without one cost nothing.

**5. Only calls.** The operand must be a call expression. This matches the intent ("do this thing") and avoids deferred expressions with no effect.

**6. Interpreter.** Each function call pushes a frame of `(expression, environment)` pairs. A `defer` adds to the top frame; when the function ends the frame is popped and its expressions are evaluated in reverse order in their environments. The interpreter evaluates the expression at exit time with the variables as they are then; the native compiler captures them when `defer` runs. For programs where variables do not change between `defer` and the exit, they agree; the documentation states the native rule, and the difference is listed as an interpreter limit.

## Risks / Trade-offs

- [A deferred call that allocates is slower than inlined cleanup] → One small closure per executed `defer`; acceptable, and only functions that use `defer` pay.
- [Sharing records can surprise people used to copy semantics] → It is what makes `defer buf.flush()` do the useful thing, and it is documented.
- [Interpreter and native differ on late variable changes] → Documented; the native behavior is the specification.
- [Defers do not run on a panic] → Documented; there is no unwinding in Zila Lang.
