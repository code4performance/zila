## MODIFIED Requirements

### Requirement: The default is unchanged
Building without `--target` or with `--target windows` SHALL produce an executable that behaves exactly as the one built before this target existed (same output, exit codes and run-time behavior), though the bytes of the `.exe` may differ because the compiler now writes it itself. The bare-metal build (`--os`) SHALL be unchanged.

#### Scenario: Regression
- **WHEN** the existing test suites for the Windows target and the operating system image run
- **THEN** all pass and the boot image is byte-for-byte the same as before
