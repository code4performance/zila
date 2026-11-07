## MODIFIED Requirements

### Requirement: Making sorted maps and sets
`use sorted` SHALL provide `SortedMap<K, V>` and `SortedSet<T>`. `sorted.int_map()` and `sorted.int_set()` SHALL order integers ascending, `sorted.text_map()` and `sorted.text_set()` SHALL order text by bytes (like `text.compare`), and `sorted.map_by(less)` and `sorted.set_by(less)` SHALL order by a function `less(a, b)` that tells whether `a` comes before `b`. The element types come from the expected type. Two keys SHALL be the same key when neither is less than the other. A sorted collection SHALL be a shared reference: it is a tree of shared nodes, so assigning it or passing it gives another name for the same collection, unlike `Map<K, V>` and `Set<T>`, which have value semantics.

#### Scenario: Ready-made and custom order
- **WHEN** a program makes `let m: sorted.SortedMap<int, text> = sorted.int_map()` and `let d: sorted.SortedSet<int> = sorted.set_by(func(a, b) a > b end)`
- **THEN** both compile, `m.len` and `d.len` are 0, and `d` lists its elements from largest to smallest

#### Scenario: Records as keys
- **WHEN** a program orders a map of records by a field with `sorted.map_by`
- **THEN** keys come out in the order of that field, and two records with the same field value are one key

#### Scenario: Shared between names
- **WHEN** a program assigns a sorted map to a second name and puts a key through one name
- **THEN** the other name sees the key
