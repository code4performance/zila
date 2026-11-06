## Context

After `add-basic-optimizer`, `codegen_x64.rs` still addresses every local as `[rbp - off]`. The loop `sum += i * 2 + 1` costs a store and a load of each variable per turn, and the processor's store-forwarding latency bounds the run time (about 210 ms for 100 million turns, against 233 ms without optimization). The compiler never uses `rbx`, `r12`, `r13`, `r14` or `r15` (only the user `asm` register table names them), and Windows x64 treats them as callee-saved, so they are free to hold variables as long as each function restores them. Fibers (`SwitchToFiber`) and `extern "system"` calls already preserve them.

## Goals / Non-Goals

**Goals**
- Keep the most used plain integer, boolean and pointer variables of each function in registers, with correct behavior in every program the compiler accepts.
- Make the 100-million-iteration loop at least 25% faster than `--no-opt`.

**Non-Goals**
- A general register allocator for temporaries, live-range splitting, floats in xmm registers, inlining. Variables are assigned for the whole function.

## Decisions

1. **Eligibility.** A local or parameter is eligible when its type is an integer, `bool` or pointer (one machine word, not reference counted, not `Ty::Param`) and it is not `captured` by a closure. Instances of generic functions are decided after substitution. A function containing `asm`, `defer`, or taking the address of a local is not allocated at all (a conservative blanket rule; `has_asm` already exists). Function bodies that are compiled in `Target::Bare` are not allocated.
2. **Choosing.** A pre-pass walks the function's statements once and counts uses per declared name, adding 8 for each loop nesting level (`while`, `for`). Names are resolved by scope as the code generator does, so shadowed variables count separately; the pass records a score per declaration (keyed by the declaring node's position). The five best declarations with a score of at least 3 get `rbx, r12, r13, r14, r15` in that order. Because the assignment is for the whole function, two variables never share a register, which avoids any liveness analysis.
3. **Where it plugs in.** `Local` gains `reg: Option<&'static str>`. The few places that read or write `[rbp - off]` for a variable (`ident`, `assign`, compound assignment, `declare_local` initialization, parameter binding, `for` loop counters, `simple_rhs`, `cond_jump`/`operand`) go through two helpers: `load_local(rax, &Local)` and `store_local(&Local)`, which emit `mov rax, reg`/`mov reg, rax` for register locals and the existing memory form otherwise. `simple_rhs` and the direct-operand path use the register directly (`add rax, r12`).
4. **Prologue/epilogue.** `finish_proc` saves each used register in a fixed save area below the other slots (`mov [rbp - n], r12`) and restores it right before `leave`/`ret` (the `ret_label` epilogue). Parameters arriving in `rcx, rdx, r8, r9` are copied into their registers by `bind_params`. Because registers are saved in memory slots (not pushed), stack alignment and the slot-zeroing loop are untouched.
5. **Calls.** Callee-saved registers survive calls by the ABI, so call sites need no spilling. The compiler's own runtime helpers (written in assembly in the generated file) are checked once: they use `rsi`, `rdi` (saved by them), `rax`, `rcx`, `rdx`, `r8-r11` only; the audit is a task and a test greps the helper text for the five registers.
6. **Exceptional paths.** `return`, `break`, `continue`, error propagation (`?`) and `defer`-free scope exits do not touch variables in registers, only release heap slots. Code that jumps to the function's `ret_label` runs the common restore sequence, so every exit restores the registers.
7. **Peephole interaction.** The peephole's store-then-reload rule matches `[X]` operands; with register locals those lines no longer exist. `mov r, r` removal is already limited to 64-bit registers. A new rule drops `mov rax, r12` followed by `mov r12, rax`.
8. **Switch.** All of it sits behind `self.opt`; `--no-opt` output must stay byte-identical to the current `--no-opt` output (checked by a test that compares against a stored hash of the asm of a few programs, or by diffing before and after during development).
9. **Testing.** The existing differential tests already compare optimized and unoptimized runs of every program; new tests add recursion and mutual calls with live register variables across calls, shadowing, parameters mutated in loops, `for` loops, `break`/`continue`, early `return`, error propagation inside loops, tasks switching with live variables, callbacks from native code, and randomized programs using five or more variables so the five-register limit and the memory fallback both run. The benchmark test gains the 25% loop assertion.

## Risks / Trade-offs

- **A register clobbered by the compiler or a helper corrupts a variable silently.** Mitigation: the audit of the helper text, the differential and random tests, and `--no-opt`.
- **Missed exit path leaves a callee-saved register changed.** Mitigation: all exits go through the one epilogue; a test calls a register-using function from a function that keeps its own values in the same registers and checks them afterwards.
- **Code size.** Each function that uses registers adds up to ten instructions in the prologue and epilogue; the threshold of three uses keeps tiny functions out.
- **Blanket exclusions (asm, defer, closures)** leave some functions slower than they could be; they can be relaxed later.

## Results (measured while implementing)

Same benchmarks and method as the optimizer change (best of three after a warm-up run; instructions are those of the benchmark's main procedure).

| Benchmark | Instructions (`--no-opt` / optimized) | Time (`--no-opt` / optimized) |
|---|---|---|
| fib | 53 / 30 | 426-500 ms / 232-254 ms |
| loop | 53 / 39 | 205-227 ms / 74-77 ms (-64% to -67%) |
| sort | 403 / 283 | 39-42 ms / 32-38 ms |
| join | 90 / 75 | 58-71 ms / 50-65 ms |

What changed from the plan while implementing:

- The use-count threshold is 8 (one use in a loop), not 3: with 3, `fib` kept `n` in a register and got slower (30 to 33 instructions, plus saving and restoring on every call).
- The counter of a `for a..b` loop is a candidate too (it is stepped and tested every turn); the 100-million-iteration loop gained little until it was, because `sum` alone left `i` in memory. Loops over lists and maps still keep their variables in memory.
- The plan is made from the syntax before code generation (`src/regalloc.rs`), over names bound exactly once in the function; the type check (integers, booleans, pointers) happens when the variable is declared. Functions with receivers, `async`, `defer`, `spawn`, `await` or `asm`, and by-reference or default parameters, are not allocated; a variable whose address is taken is not either.
