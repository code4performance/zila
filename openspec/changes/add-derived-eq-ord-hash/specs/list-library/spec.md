## ADDED Requirements

### Requirement: Sorting and searching by the standard order
`sort(xs)` SHALL return a new list holding the elements of a list whose element type is `Ord`, ordered ascending by `compare`, stable and in O(n log n) comparisons, and `contains(xs, x)` SHALL be true when some element of a list of an `Eq` type is `==` to `x`. An element type without the capability SHALL be a compile error naming the type.

#### Scenario: Sort integers, text and records
- **WHEN** a program sorts `[3, 1, 2]`, `["b", "a"]` and a list of records with derived `compare`
- **THEN** the results are `[1, 2, 3]`, `["a", "b"]` and the records in field order, and the argument is unchanged

#### Scenario: Contains
- **WHEN** a program calls `contains` on a list of records for an equal record and for a different one
- **THEN** the results are true and false

#### Scenario: Unsortable element
- **WHEN** a program sorts a list of records that hold a map field
- **THEN** compilation fails with an error that the record is not Ord
