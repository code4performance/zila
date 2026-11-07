## REMOVED Requirements

### Requirement: Interpreter
**Reason**: The interpreter is removed; `use list` runs through the native compiler under `--run` (`run-command`).
**Migration**: `tests/stdlib.rs` asserts the expected output of `examples/words.br` natively.
