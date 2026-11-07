## REMOVED Requirements

### Requirement: The interpreter agrees
**Reason**: The interpreter is removed; programs using `sorted` run natively under `--run` (`run-command`).
**Migration**: `tests/sorted.rs` runs its programs natively, including the call of a function stored in a record field.
