## REMOVED Requirements

### Requirement: Interpreter
**Reason**: The interpreter is removed; `use text` and `??` run natively under `--run` (`run-command`).
**Migration**: `tests/stdlib.rs` asserts the expected output of `examples/words.br` natively.
