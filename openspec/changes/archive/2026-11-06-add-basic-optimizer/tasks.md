## 1. Switch and baseline

- [x] 1.1 Add `CompileOptions { optimize }`, the `--no-opt` flag (command line, `cli::FLAGS`, docs flag list) and a `Compiler::opt` field that every optimization checks; verify that `--asm --no-opt` output equals today's output byte for byte and that the docs flag test passes
- [x] 1.2 Add `tests/optimizer.rs` with the differential helper (compile both ways, compare output and exit code) and the benchmark programs; record the baseline instruction counts and run times of `fib(32)`, a 100-million-iteration loop, sorting 100,000 numbers and joining 100,000 texts

## 2. Constant folding

- [x] 2.1 Implement `fold` for integer literals, unary minus, bitwise and shift operators, comparisons and `not`/`and`/`or`, with width wrapping and the zero-divisor exception; verify with assembly checks (`2 * 3 + 1` loads `7`) and the wrapping and division-by-zero scenarios
- [x] 2.2 Fold float expressions (`f64` and `f32`), including NaN, infinities and signed zero; verify against the printed results of the same expressions compiled with `--no-opt`

## 3. Fewer temporaries and tests

- [x] 3.1 Use literal and plain-local right operands directly in `binary` (the left operand stays in `rax`), keeping the slow path for everything else and for captured locals; verify with the differential suite and a check that `n - 1` has no spill
- [x] 3.2 Add `cond_jump` and use it for `if`, `while`, `and`, `or` and `not` conditions on integer comparisons; verify with the differential suite and loops over every comparison operator for signed and unsigned values
- [x] 3.3 Extend `cond_jump` to float comparisons with the NaN rules; verify every operator with NaN, infinities and signed zero in both the taken and not-taken direction

## 4. Peephole and function entry

- [x] 4.1 Add the peephole pass to `finish_proc` (store/reload, jump to next label, unreachable code, same-register move, `mov 0` to `xor` when flags are dead), skipping naked functions and `asm` blocks; verify that an `asm` block's text is unchanged and that the differential suite passes
- [x] 4.2 Add `FnState::may_own`, set at every release-registration site, and skip the slot-zeroing loop when it is false; verify that numeric functions have no zeroing loop, that functions with each heap type still zero, and that a leak test with texts, lists, maps, records, closures and results stays flat

## 5. Verification and measurement

- [x] 5.1 Add the random-expression differential test (several thousand integer and float expressions) and run the whole existing test suite against optimized code; verify everything passes, and that the OS image still builds and boots when QEMU is present
- [x] 5.2 Measure the benchmarks against the baseline and record the results in the design document; verify the thresholds of the spec (fib code at least 40% shorter, `fib` at least 25% faster, the loop not slower)

## 6. Documentation

- [x] 6.1 Document `--no-opt` in `docs/getting-started.html`, add what is optimized (and what is not yet) to `docs/native.html`, and update the status table and roadmap in `README.md`; verify `cargo test` passes, including the docs tests (flags, links, code samples)
