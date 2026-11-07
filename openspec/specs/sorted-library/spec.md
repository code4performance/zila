## Purpose

Gives programs maps and sets that keep their entries in key order (ordered by a function), with nearest-key and range queries, in logarithmic time.

## Requirements

### Requirement: Making sorted maps and sets
`use sorted` SHALL provide `SortedMap<K, V>` and `SortedSet<T>`. `sorted.int_map()` and `sorted.int_set()` SHALL order integers ascending, `sorted.text_map()` and `sorted.text_set()` SHALL order text by bytes (like `text.compare`), and `sorted.map_by(less)` and `sorted.set_by(less)` SHALL order by a function `less(a, b)` that tells whether `a` comes before `b`. The element types come from the expected type. Two keys SHALL be the same key when neither is less than the other. A sorted collection SHALL be a shared reference, like a map.

#### Scenario: Ready-made and custom order
- **WHEN** a program makes `let m: sorted.SortedMap<int, text> = sorted.int_map()` and `let d: sorted.SortedSet<int> = sorted.set_by(func(a, b) a > b end)`
- **THEN** both compile, `m.len` and `d.len` are 0, and `d` lists its elements from largest to smallest

#### Scenario: Records as keys
- **WHEN** a program orders a map of records by a field with `sorted.map_by`
- **THEN** keys come out in the order of that field, and two records with the same field value are one key

### Requirement: Putting, reading and removing in a sorted map
`put(key, value)` SHALL add the entry or replace the value of the same key and return true when the key was new. `get(key, default)` SHALL return the stored value or `default`, `has(key)` SHALL say whether the key is present, `remove(key)` SHALL remove it and return true when it was present (and do nothing otherwise), `clear()` SHALL remove every entry and `len` SHALL be the number of entries.

#### Scenario: Put, get and replace
- **WHEN** a program puts `(2, "b")`, `(1, "a")` and `(2, "B")`
- **THEN** the first two puts return true, the third false, `len` is 2, `get(2, "?")` is `B` and `get(9, "?")` is `?`

#### Scenario: Remove
- **WHEN** a program removes key 1 twice
- **THEN** the first returns true and the second false, and `has(1)` is false

### Requirement: Entries come out in key order
`keys()`, `values()` and `entries()` SHALL return lists in ascending key order (as the collection's order defines it), where `entries()` holds items with `key` and `value` fields. The lists SHALL be new lists that do not change when the collection changes later.

#### Scenario: Insertion order does not matter
- **WHEN** a program puts keys 5, 1, 4, 2, 3 in that order
- **THEN** `keys()` is `[1, 2, 3, 4, 5]`, and `values()` and `entries()` follow the same order

### Requirement: Ordered queries
`min()` and `max()` SHALL return the smallest and largest key, `floor(key)` the largest key that is not after `key`, `ceil(key)` the smallest key that is not before `key`, and each SHALL return an error when no such key exists. `range(lo, hi)` SHALL return the keys `k` with `lo <= k < hi` in order (an empty list when `hi` is not after `lo`).

#### Scenario: Nearest keys
- **WHEN** a map has keys 10, 20 and 30
- **THEN** `min()` is 10, `max()` is 30, `floor(25)` is 20, `ceil(25)` is 30, `floor(5)` and `ceil(35)` are errors, `range(10, 30)` is `[10, 20]` and `range(30, 10)` is `[]`

#### Scenario: Empty collection
- **WHEN** a sorted set is empty
- **THEN** `min()` and `max()` are errors and `to_list()` is `[]`

### Requirement: Sorted sets
A `SortedSet` SHALL provide `add(x)` (true when `x` was not in the set), `has(x)`, `remove(x)` (true when it was), `clear()`, `len`, `to_list()` in order, `min()`, `max()`, `floor(x)`, `ceil(x)` and `range(lo, hi)` with the meanings above. `sorted.union(a, b)`, `sorted.intersect(a, b)` and `sorted.difference(a, b)` SHALL return new sets ordered like `a` and SHALL NOT change their arguments, and `sorted.is_subset(a, b)` SHALL be true when every element of `a` is in `b`.

#### Scenario: Set operations in order
- **WHEN** `a` is `{3, 1, 2}` and `b` is `{2, 4}` (integer sets)
- **THEN** the union lists `[1, 2, 3, 4]`, the intersection `[2]`, `difference(a, b)` `[1, 3]`, and `a` and `b` are unchanged

### Requirement: Balanced and fast
Adding, finding and removing SHALL take time proportional to the logarithm of the number of entries, whatever the order of insertion (including ascending and descending keys).

#### Scenario: Many entries
- **WHEN** a program puts 100,000 integers in ascending order, then removes every other one
- **THEN** `len` is 50,000, every remaining key is found, `keys()` is ascending, and the program finishes within a few seconds

### Requirement: No memory leaks
A sorted collection SHALL free the memory of its nodes when it is no longer used and when entries are removed or the collection is cleared.

#### Scenario: Leak check
- **WHEN** a program builds, fills, partly empties and drops sorted maps and sets of text in a loop and then compares `live_objects()` with its value before the loop
- **THEN** the count has not grown

### Requirement: The interpreter agrees
Programs using `sorted` SHALL print the same in the interpreter (`--run`) and as native executables, including the case of calling a function stored in a record field.

#### Scenario: Same output
- **WHEN** the same program using sorted maps and sets is run both ways
- **THEN** the output is identical
