## MODIFIED Requirements

### Requirement: Run-time errors survive inlining
A division by zero, an out-of-range index or any other abort inside an inlined body SHALL behave as it does in the unoptimized program (same message, same file and line of the original source, same exit code).

#### Scenario: Division by zero in a helper
- **WHEN** an inlined `func half(a: int, b: int) -> int return a / b end` is called with `b` equal to 0
- **THEN** the program aborts with the same message, location and exit code as with `--no-opt`
