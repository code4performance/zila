## MODIFIED Requirements

### Requirement: Division by zero
An integer division or remainder by zero SHALL behave the same on both targets: the program prints `abort: <file>.br:<line>: division by zero` to standard error and exits with status 101, like any other abort.

#### Scenario: Divide by zero
- **WHEN** a program divides an integer by a variable that is zero
- **THEN** it prints `abort: <file>.br:<line>: division by zero` and exits with status 101 on both targets
