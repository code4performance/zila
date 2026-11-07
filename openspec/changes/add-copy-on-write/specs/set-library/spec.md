## MODIFIED Requirements

### Requirement: The Set type
`use set` SHALL provide a generic `Set<T>` whose element type `T` is `text` or an integer type. `set.empty()` SHALL make an empty set whose type comes from the expected type, and `set.from_list(xs)` SHALL make a set of the elements of a list, ignoring repeats. Any other element type SHALL be rejected at compile time with a message naming the type (a set keeps its elements as map keys, so the message is the map key message). A set SHALL have value semantics, like a map: assigning it or passing it copies nothing until one holder writes, and a write through one name SHALL NOT change another (a `ref` parameter or receiver refers to the caller's set). A set SHALL release its memory when it is no longer used.

#### Scenario: Valid sets
- **WHEN** a program declares `let a: set.Set<text> = set.empty()` and `let b = set.from_list([3, 1, 3])`
- **THEN** it compiles, `a.len` is 0 and `b.len` is 2

#### Scenario: Unsupported element
- **WHEN** a program declares `set.Set<Point>` where `Point` is a record
- **THEN** compilation fails with an error saying that `Point` cannot be a map key and that keys are text or integers

#### Scenario: Copy is independent
- **WHEN** a program does `var t = s` for a set `s` and calls `t.add(7)`
- **THEN** `s.has(7)` is false and `t.has(7)` is true
