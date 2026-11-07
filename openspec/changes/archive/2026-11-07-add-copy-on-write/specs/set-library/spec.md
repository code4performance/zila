## MODIFIED Requirements

### Requirement: The Set type
`use set` SHALL provide a generic `Set<T>` whose element type `T` is any type that is `Hash` (see `maps` for what qualifies). `set.empty()` SHALL make an empty set whose type comes from the expected type, and `set.from_list(xs)` SHALL make a set of the elements of a list, ignoring repeats (elements that are `==`). An element type that is not `Hash` SHALL be rejected at compile time with the map key message (a set keeps its elements as map keys). A set SHALL have value semantics, like a map: assigning it or passing it copies nothing until one holder writes, and a write through one name SHALL NOT change another (a `ref` parameter or receiver refers to the caller's set). A set SHALL release its memory when it is no longer used.

#### Scenario: Valid sets
- **WHEN** a program declares `let a: set.Set<text> = set.empty()` and `let b = set.from_list([3, 1, 3])`
- **THEN** it compiles, `a.len` is 0 and `b.len` is 2

#### Scenario: Record elements
- **WHEN** a program makes `set.from_list([P { x: 1, y: 2 }, P { x: 1, y: 2 }, P { x: 2, y: 1 }])` for a record `P` of integers
- **THEN** it compiles and `len` is 2, `has(P { x: 2, y: 1 })` is true, and `union`, `intersect` and `difference` work on such sets

#### Scenario: Unsupported element
- **WHEN** a program declares `set.Set<float>`
- **THEN** compilation fails with an error saying that `float` cannot be a map key because it is not Hash

#### Scenario: Copy is independent
- **WHEN** a program does `var t = s` for a set `s` and calls `t.add(7)`
- **THEN** `s.has(7)` is false and `t.has(7)` is true
