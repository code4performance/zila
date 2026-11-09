## ADDED Requirements

### Requirement: Output depends only on the program and the flags
Compiling the same program with the same flags and target SHALL always give the same assembly text, with the same procedures in the same order, whatever the order in which the compiler's internal tables happen to be walked.

#### Scenario: Two compiles
- **WHEN** the same file is compiled twice with the same flags and target
- **THEN** the two assembly texts are identical

#### Scenario: Another target or flags
- **WHEN** the file is compiled for `--target linux` or with `--no-opt`
- **THEN** each of those is also identical across two compiles
