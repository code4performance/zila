## Purpose

Gives programs the everyday operations on lists (transforming, searching, ordering, building) as generic functions, so they do not have to be written by hand for every element type.

## Requirements

### Requirement: Transforming
`use list` SHALL provide `map(xs, f)`, `filter(xs, keep)` and `reduce(xs, start, f)`. `map` SHALL return a new list of `f` applied to each element in order, and its element type MAY differ from the input's. `filter` SHALL keep the elements for which `keep` is true, in order. `reduce` SHALL combine from left to right: `f(f(start, x0), x1)...`. None SHALL change `xs`.

#### Scenario: Map across types
- **WHEN** a program maps `[1, 2, 3]` with `func(n) "n{n}" end`
- **THEN** the result is the list of texts `["n1", "n2", "n3"]`

#### Scenario: Filter and reduce
- **WHEN** a program filters `[1, 2, 3, 4]` for even numbers and reduces the result with `0` and addition
- **THEN** the sum is `6`

### Requirement: Searching
`any(xs, test)` and `all(xs, test)` SHALL stop at the first element that decides the answer; `all` of an empty list SHALL be true and `any` false. `find(xs, test)` SHALL return the index of the first match or `-1`.

#### Scenario: Short circuit
- **WHEN** `any` runs on a list where the second element matches
- **THEN** `test` is called exactly twice

#### Scenario: Find
- **WHEN** a program evaluates `list.find([5, 8, 12], func(n) n > 6 end)` and the same with a test nothing satisfies
- **THEN** the results are `1` and `-1`

### Requirement: Building and reordering
`reverse(xs)`, `slice(xs, from, to)` and `concat(a, b)` SHALL return new lists; `slice` SHALL clamp `from` and `to` to the list's bounds and return an empty list when `from >= to`. `range(a, b)` SHALL return the integers from `a` up to but not including `b`, empty when `b <= a`.

#### Scenario: Slice clamps
- **WHEN** a program slices `[1, 2, 3, 4]` with `from` `-5` and `to` `2`, and with `from` `3` and `to` `99`
- **THEN** the results are `[1, 2]` and `[4]`

#### Scenario: Range
- **WHEN** a program evaluates `list.range(2, 5)`
- **THEN** the result is `[2, 3, 4]`

### Requirement: Sorting
`sort_by(xs, less)` SHALL return a new list holding the elements ordered so that `less(b, a)` is false for every adjacent pair `a, b`, and it SHALL be stable: equal elements keep their original order. It SHALL take O(n log n) comparisons.

#### Scenario: Sort numbers and texts
- **WHEN** a program sorts `[3, 1, 2]` with `func(a, b) a < b end`
- **THEN** the result is `[1, 2, 3]`

#### Scenario: Stability
- **WHEN** a program sorts records `(name, age)` by age only, with two people of the same age
- **THEN** those two stay in their original order

#### Scenario: Large input
- **WHEN** a program sorts 100000 pseudo-random integers
- **THEN** the result is ordered and the call finishes in well under a second

### Requirement: Ownership
All functions SHALL follow the existing list rules for element ownership: lists of texts, records and lists SHALL be retained and released correctly, so that building and dropping results leaves `live_objects()` unchanged.

#### Scenario: No leaks
- **WHEN** a program maps, filters, sorts and reverses a list of texts 500 times
- **THEN** `live_objects()` does not grow between iterations

### Requirement: Interpreter
`--run` SHALL support `use list` with the same results as the native compiler.

#### Scenario: Same output
- **WHEN** `examples/words.br` runs natively and with `--run`
- **THEN** the output is identical
