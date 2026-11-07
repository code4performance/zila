## MODIFIED Requirements

### Requirement: Making sorted maps and sets
`use sorted` SHALL provide `SortedMap<K, V>` and `SortedSet<T>`. `sorted.map()` and `sorted.set()` SHALL make an empty collection for any key type that is `Ord`, ordered ascending by `compare` (integers by value, text by bytes like `text.compare`, records and enums by their derived or declared `compare`), and an element or key type that is not `Ord` SHALL be rejected at compile time with a message naming the type and the reason. `sorted.map_by(less)` and `sorted.set_by(less)` SHALL order by a function `less(a, b)` that tells whether `a` comes before `b`, for any key type. The element types come from the expected type. Two keys SHALL be the same key when neither is before the other. A sorted collection SHALL be a shared reference, like a map. `int_map`, `text_map`, `int_set` and `text_set` SHALL no longer exist.

#### Scenario: Ready-made and custom order
- **WHEN** a program makes `let m: sorted.SortedMap<int, text> = sorted.map()` and `let d: sorted.SortedSet<int> = sorted.set_by(func(a, b) a > b end)`
- **THEN** both compile, `m.len` and `d.len` are 0, and `d` lists its elements from largest to smallest

#### Scenario: Text keys
- **WHEN** a program makes `sorted.map()` for text keys and puts `"b"`, `"a"`, `"c"`
- **THEN** `keys()` is `["a", "b", "c"]`

#### Scenario: Records as keys
- **WHEN** a program makes a `sorted.SortedMap<Version, text>` with `sorted.map()` for a record `Version` of integers (major, minor, patch)
- **THEN** keys come out ordered by major, then minor, then patch, and two equal versions are one key

#### Scenario: Declared order
- **WHEN** `Task` declares `compare` by priority and a `sorted.SortedSet<Task>` is made with `sorted.set()`
- **THEN** `to_list()` follows that order

#### Scenario: Custom order for any type
- **WHEN** a program orders a map of records by a field with `sorted.map_by`
- **THEN** keys come out in the order of that field, and two records with the same field value are one key

#### Scenario: Key without an order
- **WHEN** a program makes `sorted.map()` with a record that holds a map field as the key type
- **THEN** compilation fails with an error that the record is not Ord because that field is not Ord

#### Scenario: Removed constructors
- **WHEN** a program calls `sorted.int_map()`
- **THEN** compilation fails with an error that `sorted` has no function `int_map`
