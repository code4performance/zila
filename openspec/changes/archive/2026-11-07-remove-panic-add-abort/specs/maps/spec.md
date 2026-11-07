## MODIFIED Requirements

### Requirement: Reading and writing by key
`m[key]` SHALL read the value stored under `key`, and `m[key] = value` SHALL store it, adding the key when absent and replacing the value when present. Reading a missing key SHALL abort the program with the message `map key not found` (use `has` or `get(key, default)` when the key may be absent).

#### Scenario: Insert and read
- **WHEN** a program sets `m["x"] = 5` and then prints `m["x"]`
- **THEN** it prints `5`

#### Scenario: Replace
- **WHEN** a program sets `m["x"] = 5` and then `m["x"] = 6`
- **THEN** `m["x"]` is `6` and `m.len` is 1

#### Scenario: Missing key
- **WHEN** a program reads `m["nope"]` and the key was never stored
- **THEN** the program aborts with `abort: <file>.br:<line>: map key not found` and exits with 101
