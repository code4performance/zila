## 1. Sorted map

- [x] 1.1 Create `std/sorted.br` with the node, map and entry records, the constructors (`map_by`, `int_map`, `text_map`) and the AVL helpers (height, rotations, balance)
- [x] 1.2 `put`, `get`, `has`, `remove`, `clear`, `len`
- [x] 1.3 Ordered queries and listings: `keys`, `values`, `entries`, `min`, `max`, `floor`, `ceil`, `range`

## 2. Sorted set

- [x] 2.1 `SortedSet`, its constructors (`set_by`, `int_set`, `text_set`) and methods (`add`, `has`, `remove`, `clear`, `len`, `to_list`, `min`, `max`, `floor`, `ceil`, `range`)
- [x] 2.2 `union`, `intersect`, `difference`, `is_subset`

## 3. Registration and interpreter

- [x] 3.1 Register `sorted` in `std_source` and in the interpreter's library loader
- [x] 3.2 Make the interpreter call a function stored in a record field

## 4. Tests, example and documentation

- [x] 4.1 `tests/sorted.rs`: every scenario of the spec natively, and in the interpreter where it applies
- [x] 4.2 Balance and speed (100,000 ascending inserts and removals), a random-operations test against a list model, and the leak test
- [x] 4.3 `examples/sorted.br` and a section in `docs/stdlib.html`
- [x] 4.4 README rows (std libraries, roadmap), full test suite and `openspec validate add-sorted-collections`
