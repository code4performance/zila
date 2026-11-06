## Why

The compiler's code generator is deliberately simple: every sub-expression goes through a stack slot, a comparison is turned into a 0 or 1 and then tested again, constants are loaded and stored before they are used, and every function zeroes all its local slots on entry. `fib(n - 1)` compiles to about 20 instructions for what could be 3. Programs are correct but several times slower and larger than they need to be, which matters for the sorting, parsing and number-crunching the new libraries make possible. This is the optimizer item of the roadmap; this change takes the cheap, safe wins first and leaves register allocation and inlining for later.

## What Changes

- **Constant folding**: integer, float and bool expressions made only of literals are computed by the compiler (`2 * 3 + 1` becomes `7`, `-(1.5)` becomes `-1.5`, `not true` becomes `false`). Results are exactly what the running program would have computed (wrapping integer arithmetic, IEEE floats). A division or remainder by a constant zero is never folded, so it still fails at run time as before.
- **Direct operands**: when the right-hand side of a binary operation is a literal or a plain local variable, it is used directly (`sub rax, 1`, `cmp rax, [rbp - 8]`) instead of being spilled to a temporary slot and reloaded.
- **Compare and branch**: `if`, `while` and `and`/`or` conditions that are comparisons jump on the flags directly (`cmp rax, rcx` / `jge label`) instead of building a 0 or 1 and testing it. Float comparisons keep their NaN rules.
- **Peephole cleanups** on each function's generated code: a reload of a value just stored, a `jmp` to the next instruction, unreachable code after an unconditional jump, `mov rax, rax`, and `mov rax, 0` as `xor eax, eax`.
- **Cheaper function entry**: local slots are zeroed only in functions that can own a heap value (the zeroing exists so scope exits can release what the slots hold); functions that handle only numbers and booleans skip it.
- **`--no-opt`** command-line flag: compile without any of the above, for debugging and for comparing results. Optimization is on by default.
- **Measurement**: a benchmark set (`fib`, a numeric loop, sorting, text building) with the instruction counts and run times recorded in the design, so later work (register allocation, inlining) has a baseline.
- Docs (`getting-started.html` flag list, `native.html` performance notes), tests, and the README status table and roadmap are updated in the same change.

## Capabilities

### New Capabilities
- `optimizer`: what the optimizer may and may not change (behavior must be identical), the folding rules, the `--no-opt` flag, and measurable minimum improvements.

### Modified Capabilities
<!-- None: no language behavior changes. -->

## Impact

- **Compiler** (`src/codegen_x64.rs`): a constant folder over expressions (before code generation of an expression), fast paths in `binary` and in conditions, a peephole pass applied in `finish_proc`, a flag on the compiler to turn all of it off, and a "may own heap values" marker on the function state.
- **Command line** (`src/cli.rs`, `src/main.rs`, `src/docs.rs` keyword and flag lists): the `--no-opt` flag; `compile_to_native` gets an options variant (the existing function keeps its behavior and optimizes).
- **Tests**: a new `tests/optimizer.rs` with differential tests (the same programs compiled with and without optimization must print the same output), folding tests on the generated assembly, and benchmark checks; every existing test suite also runs against optimized code, since that is now the default.
- **Docs**: `docs/getting-started.html`, `docs/native.html`, `README.md`.
- **Assumptions** (no clarification needed):
  - Register allocation, inlining, loop optimizations, common-subexpression elimination and any change to the calling convention are out of scope; the design lists them as the follow-up.
  - Integer overflow keeps wrapping (the compiler does not check it today), so folding wraps too.
  - The generated assembly text is the unit the peephole works on, with a conservative rule set: every pattern is local to a few adjacent instructions and never crosses a label.
  - The bare-metal target gets the same optimizer; its tests (image builds, QEMU boot when available) must keep passing.
