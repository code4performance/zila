## REMOVED Requirements

### Requirement: The interpreter agrees
**Reason**: The interpreter is removed; `set` runs natively under `--run` (`run-command`).
**Migration**: `tests/sets.rs` runs `examples/sets.br` natively and checks the lines.
