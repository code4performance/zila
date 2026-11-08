## Context

`Map` is built in and keeps insertion order with text or integer keys; `std/set.br` wraps a map. Libraries such as `list`, `text`, `format` and `set` are Zila Lang source files embedded with `include_str!` (`std_source`) and loaded by the interpreter. A quick experiment showed that a recursive generic record with optional children (`left: Node<K, V>?`) and a record field of function type compile natively when the records are `#[shared]` (without it records are copied when passed to functions, so updates are lost). In a generic body `a < b` on a type parameter is rejected, so the order has to come from a function. The interpreter fails on `t.less(a, b)` ("no method 'less' found on record").

## Goals / Non-Goals

**Goals**
- O(log n) sorted map and set with a simple, auditable implementation in Zila Lang.
- Keys of any type, ordered by a user function, with ready-made int and text orders.

**Non-Goals**
- Iterating a sorted collection with `for k, v in m` (the language only iterates built-ins; `keys()`/`entries()` return lists), indexing with `m[k]`, or a persistent (immutable) tree.
- Making `<` work on type parameters (separate language change).

## Decisions

1. **AVL tree.** `#[shared] type Node<K, V>` holds `key`, `value`, `left`, `right` (`Node<K, V>?`) and `height`; `#[shared] type SortedMap<K, V>` holds `root`, `less` and `size`. Insertion and removal are recursive functions that return the new subtree root after rotations, so a recursion depth is at most about 1.44 log2 n. Rotations and height updates are small helper functions.
2. **Order and equality.** `less(a, b)` decides; the keys are equal when `not less(a, b) and not less(b, a)`. `int_map`/`int_set` use `a < b`; `text_map`/`text_set` use `text.compare(a, b) < 0` (`use text` inside the module).
3. **Queries.** `get`/`has` walk down with `less`; `min`/`max` walk to the leftmost and rightmost node; `floor` and `ceil` walk down remembering the best candidate; `range(lo, hi)` is an in-order walk that skips subtrees outside the interval. `keys()`, `values()` and `entries()` are in-order walks into new lists. `entries()` returns `[]Entry<K, V>` where `Entry` is a small record with `key` and `value`.
4. **Errors.** `min`, `max`, `floor` and `ceil` return `K or error` with messages such as "empty collection" and "no key at or below ..." (the message does not try to print the key, which has no known type).
5. **Sets.** `SortedSet<T>` is a shared record holding a `SortedMap<T, bool>`; the set functions walk `to_list()` of one set and probe the other, creating results with the first set's `less`.
6. **Memory.** The nodes are reference counted shared records, so removing a node or dropping the root releases the subtree. A test with `live_objects()` guards against leaks and against cycles (there are no parent pointers).
7. **Interpreter.** `src/interp.rs` loads `sorted` like the other libraries and gets one fix: a call whose callee is a field access on a record, when the record has no such method but a field holding a function, calls that function (as the native backend does).
8. **Tests.** A native/interpreter test file checks every scenario of the spec, ordering by a record field, descending order, 100,000 ascending inserts and removals (to prove the tree is balanced), a random-operations test against a reference model made with a plain list, and leaks.

## Risks / Trade-offs

- Deep recursion in a very unbalanced tree would be a risk, so a test inserts ascending and descending keys.
- A comparison function that is not a strict order (for example `<=`) gives unspecified placement; the documentation says so.
- Recursive generic code is a heavier use of generics than the existing libraries; if the compiler has a gap the fix belongs in this change.
