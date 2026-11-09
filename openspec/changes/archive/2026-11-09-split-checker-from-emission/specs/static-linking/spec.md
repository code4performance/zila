## ADDED Requirements

### Requirement: Reorganising the compiler does not change its output
A change that only reorganises the compiler's internals SHALL leave the assembly of every program in `examples/` and `examples/packages/` unchanged, for the default build, `--target linux` and `--no-opt`, and the repository SHALL keep a script that takes and compares such snapshots.

#### Scenario: A refactor
- **WHEN** a snapshot is taken before and after a change that is meant to be internal only
- **THEN** the comparison reports no difference

#### Scenario: A real change of output
- **WHEN** the compiler's output for an example changes
- **THEN** the comparison names the file and the target that differ
