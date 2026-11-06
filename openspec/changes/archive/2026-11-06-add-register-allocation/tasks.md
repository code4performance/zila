## 1. Groundwork

- [x] 1.1 Audit the generated runtime helpers and the inline `asm` register table for any use of `rbx`, `r12`-`r15`; add a test that fails if a helper uses them
- [x] 1.2 Add `reg: Option<&'static str>` to `Local` and the helpers `load_local` and `store_local`; route every read and write of a variable through them, with output unchanged

## 2. Choosing variables

- [x] 2.1 Write the use-counting pass (loop nesting weight, per-declaration scores) and the eligibility rules (type, not captured, no `asm`/`defer` in the function, not bare-metal)
- [x] 2.2 Assign the best five declarations to `rbx, r12, r13, r14, r15` when `opt` is on and record them per function

## 3. Code generation

- [x] 3.1 Bind parameters and initialize declared variables into their registers; `for` counters and assignments, compound assignments and the direct-operand and compare-and-branch paths use the register directly
- [x] 3.2 Save used registers in the prologue and restore them in the shared epilogue; check every exit path goes through it
- [x] 3.3 Add the peephole rule for `mov rax, reg` / `mov reg, rax`

## 4. Tests

- [x] 4.1 `--no-opt` keeps every variable in memory (test: no variable register appears in its output)
- [x] 4.2 Differential tests: recursion, nested calls, shadowing, mutated parameters, `for`, `break`/`continue`, early `return`, errors in loops, six or more variables, tasks switching (callbacks from native code are not tested: the ABI makes the registers callee-saved and no test program receives a callback)
- [x] 4.3 Assembly tests: the loop body of the benchmark has no stack access for its variables; floats, text, captured variables and functions with `asm`/`defer` stay in memory
- [x] 4.4 Raise the loop assertion in the benchmark test to at least 25% faster and record the numbers in the design document

## 5. Documentation

- [x] 5.1 Update `docs/native.html` (optimizer section) and the README status and roadmap rows
- [x] 5.2 Run the full test suite and `openspec validate add-register-allocation`
