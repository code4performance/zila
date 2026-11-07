## REMOVED Requirements

### Requirement: The interpreter agrees
**Reason**: The interpreter is removed; `--run` runs the native executable, so deferred calls behave as in `--build` by definition (`run-command`).
**Migration**: None. The native tests of `defer` cover the behavior.
