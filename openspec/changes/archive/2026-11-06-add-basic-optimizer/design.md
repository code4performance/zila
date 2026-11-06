## Context

`codegen_x64.rs` generates MASM text one expression at a time: `expr` leaves a value in `rax`, `binary` evaluates the left side, stores it in a temporary slot, evaluates the right side, moves it to `rcx` and reloads the left side into `rax`. Comparisons build a 0 or 1 (`setcc`, `movzx`) and `if` then does `test rax, rax; jz`. Constants go `mov rax, imm; mov [slot], rax; mov rcx, rax`. `finish_proc` wraps a function's code with a prologue that zeroes every slot, because scope exits release whatever the slots hold. The output goes through `ml64`, which does no optimization of its own. The `--asm` flag prints the result, which makes before/after comparison easy.

## Goals / Non-Goals

**Goals**
- Large, safe reductions in instructions and memory traffic for ordinary code, with every transformation easy to argue correct.
- A switch (`--no-opt`) and a differential test suite so a wrong transformation is found quickly.

**Non-Goals**
- Register allocation, inlining, loop optimizations, common-subexpression elimination, a new intermediate representation. They are the next step; the numbers recorded here are their baseline.

## Decisions

1. **Where each optimization lives.** Folding and operand/branch fast paths are done while generating code, where the types and the expression tree are known. The peephole works afterwards on the finished text of one function. This keeps each piece small and independently switchable (`Compiler::opt: bool`).
2. **Constant folding** is a function `fold(&Expr) -> Option<Const>` where `Const` is `Int(i64, Ty)`, `Float(f64, bits)` or `Bool`. It covers literals, unary minus and `not`, binary operators on two foldable operands, parentheses, and `if` expressions with a constant condition are not folded (kept simple). Integer results are wrapped to the width and signedness of the type the expression would have at run time (the same rules `norm` applies), shifts use the masked count the processor uses, and a division or remainder with a zero divisor returns `None`. Float folding uses Rust's `f64` operations, which are the same IEEE operations the SSE instructions perform; `f32` is computed in `f32`. The folder runs at the top of `expr` for `Binary` and `Unary` nodes, so folded values never reach the code that would emit them.
3. **Direct operands.** In `binary`, when the right operand is a literal that fits a 32-bit immediate or a plain local variable of a non-rc type, the code is: evaluate the left side into `rax`, then `op rax, imm` or `op rax, [rbp - off]` (for `/` and `%`, load into `rcx`). No temporary slot is used. Operators that need `cl` (shifts) and float operations keep `rcx` as before (`mov rcx, imm`). The existing slow path remains for every other right operand.
4. **Compare and branch.** A helper `cond_jump(&Expr, false_label)` is used by `if`, `while` and the short-circuit operators: for a comparison of two integers (or an integer and a literal) it emits `cmp` and the inverse conditional jump; for a float comparison it emits `ucomisd` and the jump sequence that keeps NaN false (the same flag logic as `float_binary`, written with `jp`/`jne` combinations); for `not x`, `and` and `or` it recurses with swapped labels; everything else evaluates the value and does `test rax, rax; jz`. The value-producing path (`let b = x < y`) is unchanged.
3b. **Why these are safe.** Each fast path emits exactly what the slow path computes for operands that have no side effects and cannot be changed by evaluating the other side (a literal, or a local variable that the left expression does not assign, which an expression cannot do in Brasa because assignment is a statement). Calls inside the left operand can change a captured variable only through closures; locals captured by closures are marked `captured` and take the slow path.
5. **Peephole** runs in `finish_proc` over `fs.code` lines with a small set of rewrites applied until nothing changes: `mov [S], rax` followed by `mov rax, [S]` drops the reload; `jmp L` followed by `L:` drops the jump; instructions between an unconditional `jmp` or `ret` and the next label are dropped; `mov r, r` is dropped; `mov eax, 0`/`mov rax, 0` becomes `xor eax, eax` only when the flags are not read afterwards (checked by looking at the next instruction). Lines are matched as text; any line that contains a label, an `asm` block marker or a `call` ends a window. Naked functions and inline assembly are skipped entirely (the code lines of an `asm` block are marked when emitted so the pass can leave them alone).
6. **Slot zeroing.** `FnState` gets `may_own: bool`, set when anything registers a slot for release (an `owned` entry of an rc type, `stmt_temps`, `defer_list`, parameters of rc type, closure captures). `finish_proc` omits the zeroing loop when it is false and `opt` is on. If the flag is ever missed, the observable symptom is a release of a garbage slot, which the leak and differential tests exercise heavily (every rc-using test program is compiled both ways).
7. **Flag plumbing.** `Compiler::new` takes `opt: bool` through a new `CompileOptions { optimize: bool }`; `compile_to_native` keeps its signature and calls `compile_to_native_with(file, out, &CompileOptions::default())`. `--no-opt` is added to `cli::FLAGS` (the docs test requires it to be documented) and `generate_asm_with` serves `--asm`. The bare-metal builder uses the default options.
8. **Differential testing.** `tests/optimizer.rs` compiles each program twice and compares output and exit code. A generator makes random expressions (ints of several widths and signedness, floats, nested comparisons, `and`/`or`/`not`, shifts, wrapping) printed by one program per batch.
9. **Measurement.** The benchmark test records instruction counts of the generated assembly (lines of instructions inside the function) and wall-clock times (best of three), and prints them with `--nocapture`; the thresholds are those of the spec. The initial numbers go into this document's "Results" section when implementation finishes.

## Risks / Trade-offs

- **A wrong rewrite is a silent miscompile.** Mitigation: tiny local rules, `--no-opt`, differential tests over every existing test program (the suites run optimized by default) plus random expressions.
- **Float branch fusion and NaN.** Mitigation: reuse the exact flag logic of `float_binary` and test every operator with NaN, infinities and signed zero in both branches.
- **Missed `may_own` marking** causes a crash, not wrong output. Mitigation: set it at every release-registration site (found by searching for the helpers that register releases) and add leak tests with each rc type in a numeric-looking function.
- **Text-based peephole** is fragile to formatting changes in the generator. Mitigation: patterns are matched on the exact strings the generator emits, covered by tests that fail loudly when a pattern stops matching (the instruction-count thresholds).
- **Gains are bounded** until values live in registers. Accepted: this change is the baseline for that work.

## Results (measured while implementing)

Instruction counts are the instructions inside the benchmark's main procedure; times are the best of three runs after one warm-up run, on the development machine (`fib` is `fib(38)`, `loop` is 100 million iterations of `sum += i * 2 + 1`, `sort` sorts 100,000 integers, `join` joins and splits 100,000 texts).

| Benchmark | Instructions before | after | Time before | after |
|---|---|---|---|---|
| fib | 53 | 30 (-43%) | 424-466 ms | 258-273 ms (-36% to -45%) |
| loop | 53 | 36 (-32%) | 233-240 ms | 209-216 ms (-10%) |
| sort | 403 | 277 (-31%) | 38-47 ms | 33-36 ms (-10% to -22%) |
| join | 90 | 74 (-18%) | 49-65 ms | 53-56 ms (no clear change) |

Two observations that shape the next step: (1) timings that include process start-up are meaningless for short programs (the first run of a new executable costs over 100 ms here), so the benchmark warms up first; (2) the numeric loop is limited by the dependency through memory: `sum` and `i` are stored and reloaded on every turn, which costs about 5 cycles each however few instructions surround them. Only keeping variables in registers removes that, so a 25% gain on such loops is a register-allocation result, not a result of this change.

Bug found by the random-expression test while building: the peephole rule that removes `mov r, r` must not remove `mov eax, eax`, which clears the upper half of the register (it is how 32-bit unsigned values are normalized).
