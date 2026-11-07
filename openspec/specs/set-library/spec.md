## Purpose

Gives programs a set of values (membership, union, intersection, difference) so they do not have to use a map with unused values.

## Requirements

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

### Requirement: Adding, testing and removing
A set SHALL provide `add(x)` (returns true when `x` was not yet in the set), `has(x)`, `remove(x)` (returns true when `x` was in the set, and does nothing otherwise), `clear()` and a `len` property.

#### Scenario: Add and has
- **WHEN** a program adds `"a"` twice and `"b"` once to a set of text
- **THEN** the first add returns true, the second false, `len` is 2, `has("a")` is true and `has("z")` is false

#### Scenario: Remove
- **WHEN** a program removes `"a"` and then `"a"` again
- **THEN** the first remove returns true, the second false, and `len` drops by one only once

#### Scenario: Many elements
- **WHEN** a program adds 100,000 integers
- **THEN** `len` is 100,000 and every one of them is found by `has`

### Requirement: Combining sets
`union(a, b)`, `intersect(a, b)` and `difference(a, b)` (the elements of `a` that are not in `b`) SHALL return new sets and SHALL NOT change their arguments. `is_subset(a, b)` SHALL be true when every element of `a` is in `b`, so the empty set is a subset of every set.

#### Scenario: Operations
- **WHEN** `a` is `{1, 2, 3}` and `b` is `{3, 4}`
- **THEN** the union has 4 elements, the intersection is `{3}`, `difference(a, b)` is `{1, 2}`, `is_subset(intersect(a, b), a)` is true and `a` and `b` are unchanged

### Requirement: Listing
`to_list()` SHALL return the elements as a list in the order they were first added (a removed and re-added element counts as added again), so that programs can write `for x in s.to_list()` and get the same order every run.

#### Scenario: Order
- **WHEN** a program adds `"c"`, `"a"`, `"b"`, `"a"`
- **THEN** `to_list()` is `["c", "a", "b"]`
