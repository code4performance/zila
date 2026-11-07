## RENAMED Requirements

- FROM: `### Requirement: Maps are shared references`
- TO: `### Requirement: Maps have value semantics`

## MODIFIED Requirements

### Requirement: Maps have value semantics
Assigning a map to another variable or passing it to a function SHALL give the new holder an independent value without copying the entries: the entries are copied by the first write to a map that another holder still has (see `copy-on-write`). A change made through one name SHALL NOT be visible through the others, except through a `ref` parameter or receiver, or when the map is held by a value created with `new` or `shared`.

#### Scenario: Not changed through a parameter
- **WHEN** a function receives a map (without `ref`) and sets a key, and the caller then reads that key
- **THEN** the caller does not see the new key

#### Scenario: Changed through a parameter
- **WHEN** a function receives `ref m: Map<text, int>` and sets a key, and the caller then reads that key
- **THEN** the caller sees the new value

#### Scenario: Copy is independent
- **WHEN** a program does `var b = a` for a map `a` and sets `b["x"] = 1`
- **THEN** `a.has("x")` is false and `b["x"]` is 1
