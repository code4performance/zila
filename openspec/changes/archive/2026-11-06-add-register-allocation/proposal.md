## Why

The optimizer (`add-basic-optimizer`) removed most temporaries, but every variable still lives in a stack slot: a loop such as `sum += i * 2 + 1` stores and reloads `sum` and `i` on every turn, and its speed is bound by that memory traffic. The 100-million-iteration loop gained only about 10% from the optimizer, against the 25% the first spec asked for. Keeping the hottest variables in registers is the step that makes loops fast.

## What Changes

- A function's plain integer, boolean and pointer locals (and parameters) can be kept in the callee-saved registers `rbx`, `r12`, `r13`, `r14` and `r15`, which the compiler does not use for anything else, instead of in `[rbp - n]` slots.
- Variables are chosen by use count, with uses inside loops weighted more; at most five per function. Floats, text, lists, records and other heap or multi-word values stay in memory.
- A variable stays in memory when its address is taken, when a closure captures it, when the function contains an `asm` block or a `defer`, or when it is declared in a function that has no loop and fewer than three uses.
- A function saves the registers it uses in its prologue and restores them in its epilogue, so callers and tasks (fibers) are unaffected.
- `--no-opt` keeps every variable in memory, exactly as today.
- The loop benchmark and the optimizer test set run with and without the change and must agree.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `optimizer`: adds requirements for keeping variables in registers and for the loop benchmark speedup.

## Impact

- `src/codegen_x64.rs`: a use-count pass over each function body, a register map on `FnState`/`Local`, and changes to how locals are loaded, stored, passed to calls and spilled around calls; prologue/epilogue save and restore.
- `src/cli.rs`/`main.rs`: none (`--no-opt` already exists).
- `tests/optimizer.rs`: new differential and benchmark tests; the loop threshold becomes at least 25% faster.
- `docs/native.html` and the README: describe the new optimization.
- The bare-metal target is unaffected: it keeps the memory-only code.
