## REMOVED Requirements

### Requirement: Interpreter
**Reason**: The interpreter is removed; `--run` uses the native compiler and so runs generic code exactly as `--build` does (`run-command`).
**Migration**: `tests/generics.rs` asserts the expected output of `examples/generics.br` natively.
