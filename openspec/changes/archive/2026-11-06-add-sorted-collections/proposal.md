## Why

Brasa's `Map` and `set.Set` keep entries in the order they were added and only accept text or integer keys. Programs often need the entries in key order (a leaderboard, a schedule, a dictionary listing), the smallest or largest key, the nearest key to a value, or all keys in a range, and they have to sort a list over and over to get that. The first roadmap item for the standard library is sorted maps and sets.

## What Changes

- A new library `use sorted`, written in Brasa, with `sorted.SortedMap<K, V>` and `sorted.SortedSet<T>`, both balanced binary search trees (AVL) so that adding, finding and removing take O(log n) time.
- The order comes from a function `less(a, b)` given when the collection is made, so keys can be any type the function can compare (text, integers, floats, records). Ready-made constructors cover the common cases: `sorted.int_map()`, `sorted.text_map()`, `sorted.int_set()`, `sorted.text_set()`, and `sorted.map_by(less)`, `sorted.set_by(less)` for the rest. Two keys are the same when neither is less than the other.
- `SortedMap` methods: `put(key, value)`, `get(key, default)`, `has(key)`, `remove(key)`, `clear()`, `len`, and the ordered ones `keys()`, `values()`, `entries()` (in key order), `min()`, `max()`, `floor(key)` (the largest key at most `key`), `ceil(key)` (the smallest key at least `key`) and `range(lo, hi)` (keys from `lo` up to but not including `hi`). `min`, `max`, `floor` and `ceil` return an error when there is no such key.
- `SortedSet` methods: `add`, `has`, `remove`, `clear`, `len`, `to_list` (in order), `min`, `max`, `floor`, `ceil`, `range`, and the functions `union`, `intersect`, `difference` and `is_subset`.
- Both are shared references (like `Map` and `set.Set`), free their memory when no longer used, and work in the interpreter the same way as in native code. For that the interpreter learns to call a function stored in a record field (`t.less(a, b)`).

## Capabilities

### New Capabilities

- `sorted-library`: the sorted map and sorted set (construction, ordering, lookup, ordered queries, combining sets, memory, interpreter parity).

### Modified Capabilities

None.

## Impact

- New `std/sorted.br`, registered in `std_source` (`src/codegen_x64.rs`) and in the interpreter's library loader (`src/interp.rs`), which also gains calls of function-valued record fields.
- New `tests/sorted.rs`, `examples/sorted.br`, a section in `docs/stdlib.html`, README rows (std libraries, roadmap).
