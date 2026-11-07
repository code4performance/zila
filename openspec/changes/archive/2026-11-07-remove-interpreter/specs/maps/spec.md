## REMOVED Requirements

### Requirement: The interpreter agrees
**Reason**: There is no interpreter; maps under `--run` are the native maps, including the `map key not found` panic and exit status 101 (`run-command`).
**Migration**: `tests/maps.rs` keeps the native assertion of the same lines and message.
