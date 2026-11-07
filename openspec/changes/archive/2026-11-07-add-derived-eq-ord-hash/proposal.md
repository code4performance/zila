## Why

Brasa has no common vocabulary for "these two values are equal", "this one comes first" and "this value can be a key". The consequences are scattered through the language and the library: `Map<K, V>` accepts only `text` or integer keys (a record, an enum or a list cannot be a key, so programs invent a text key by formatting the value), `std/set.br` inherits the same limit, `sorted.SortedMap` and `list.sort_by` need a hand-written `less` function every time, interfaces are structural with methods only so there is no way to say "a type that has `==`", and generic code cannot use `==` or `<` on a `T` at all (`func max<T>(a: T, b: T)` cannot be written). Records and enums, whose equality and order are obvious from their fields and variants, have to repeat that boilerplate by hand.

## What Changes

- Three standard interfaces, available without `use`: `Eq` (`func eq(self, other: Self) -> bool`), `Ord` (`func compare(self, other: Self) -> int`, negative, zero or positive) and `Hash` (`func hash(self) -> long`). Interfaces gain the name `Self` for "the type that satisfies me". A bound `Ord` or `Hash` implies `Eq`, so one bound is enough in generic code.
- The compiler **derives** `eq`, `compare` and `hash` for records and enums, field by field and variant by variant, written as comptime code on top of `add-comptime-reflection`. Defining the method on the type replaces the derived one. A type that defines `eq` but not `hash` is not `Hash` (no silently inconsistent keys).
- Rules per type: `text` by content (bytes), `float` is `Eq` and `Ord` but not `Hash` (NaN), types held by reference (`new T`, `shared T`, `#[shared]` records) compare and hash by identity unless the type defines `eq`/`hash`, lists and optionals derive `Eq`/`Ord`/`Hash` from their element, maps derive `Eq`/`Hash` from key and value, enums order by variant order then payload.
- The operators `==`, `!=`, `<`, `<=`, `>`, `>=` stay operators. On records, enums, lists, maps, optionals and on a type parameter bounded by `Eq`/`Ord` they call `eq` and `compare` (derived or user-written). A type with no `Eq` or `Ord` is a compile error naming the type and the reason.
- `Map<K, V>` accepts any key type that is `Hash` (and so `Eq`). The runtime map takes a hash function and an equality function from the compiler for keys other than `text` and integers; text and integer keys keep their fast paths.
- `set.Set<T>` accepts any `T: Hash`. `sorted.SortedMap<K, V>` and `SortedSet<T>` get constructors `sorted.map()` and `sorted.set()` for any `K: Ord`; `map_by`/`set_by` stay for custom orders. `int_map`, `text_map`, `int_set` and `text_set` are removed (**BREAKING**, replaced by `sorted.map()`/`sorted.set()`). `list.sort<T: Ord>` and `list.contains<T: Eq>` are added.
- The std, `examples/`, tests and docs that mention the old key restriction are rewritten in this change.

## Capabilities

### New Capabilities

- `standard-interfaces`: `Eq`, `Ord`, `Hash`, `Self`, derivation, overriding, the per-type rules and the operators.

### Modified Capabilities

- `interfaces`: `Self` in method signatures; a type also satisfies a standard interface through derivation.
- `generics`: operators on a type parameter are allowed when its bound provides them.
- `maps`: key type is any `Hash` type.
- `set-library`: element type is any `Hash` type.
- `sorted-library`: keys ordered by `Ord`, new constructors, removed ready-made ones.

## Impact

- `src/codegen_x64.rs`: `map_type` (key check), `new_map` / `map_method` / index and literal paths (hash and eq thunks), `binary_core` (the `Ty::Param` operator check and the operator paths for records, enums, lists, optionals), interface satisfaction and bound checks, the derivation hook; `src/runtime_map.asm` (`rt_map_new`, `rt_map_hash`, `rt_map_find`, release of keys); `src/interp.rs` (`MapVal::key_of`) only if `remove-interpreter` has not landed.
- `std/set.br`, `std/sorted.br`, `std/list.br`, a new `std/derive.br` (derivation helpers, `hash_combine`).
- Tests: `tests/maps.rs`, `tests/sets.rs`, `tests/sorted.rs`, `tests/floats.rs`, `tests/linux.rs`, `tests/generics.rs`, new `tests/derive.rs`. Examples: `examples/sorted.br`, `examples/maps.br`, `examples/sets.br`. Docs: `docs/syntax.html` (Maps, interfaces), `docs/stdlib.html`, README.
