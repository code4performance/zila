## REMOVED Requirements

### Requirement: Interpreter agreement
**Reason**: There is no interpreter; `--run` compiles natively, so `f32` and `use math` work under it too (`run-command`).
**Migration**: `tests/floats.rs` asserts the expected text of `examples/floats.br` on the native executable.
