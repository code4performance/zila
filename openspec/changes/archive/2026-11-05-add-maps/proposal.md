## Why

Maps are parsed (`{ "a": 1 }`, `Map<text, int>`) but the native compiler rejects them, so a Zila Lang program has no way to look something up by name: word counts, caches, configuration, headers of a request. Lists are the only collection, and searching one by hand is slow and noisy. A built-in map is the next most useful building block after lists, and it is the foundation the text and collection libraries on the roadmap will use.

## What Changes

- **Type**: `Map<K, V>` becomes a native type, usable in variables, parameters, return types, record fields and lists. Keys are `text` or integer types; values may be any supported type.
- **Literals**: `{ "a": 1, "b": 2 }` builds a map; `{}` needs a type annotation, as an empty list does. Key and value types are inferred from the first pair and checked on the rest.
- **Lookup and update**: `m[key]` reads, `m[key] = value` inserts or replaces. Reading a missing key is a runtime panic, like an out-of-range list index.
- **Methods and property**: `m.has(key)`, `m.get(key, default)`, `m.remove(key)`, `m.clear()`, and the property `m.len` (like `list.len` and `text.len`).
- **Iteration**: `for key, value in m` and `for key in m`, in insertion order (so output is deterministic).
- **Runtime**: a hash table written in assembly in the generated runtime, reference counted like lists and texts, releasing keys and values it owns.
- **Interpreter**: `--run` supports the same maps and behaves the same.
- Docs, an `examples/maps.br` program and the README status/roadmap are updated in the same change.

## Capabilities

### New Capabilities
- `maps`: the `Map<K, V>` type, map literals, lookup, update, removal, the methods above, iteration order, memory behavior and error cases.

### Modified Capabilities
<!-- None: lists and the other types behave as before. -->

## Impact

- **Compiler** (`src/codegen_x64.rs`): new `Ty::Map`, resolution of `Map<K, V>`, map literals, index read/write, methods, `for` over maps, drop and release.
- **Runtime** (in `codegen_x64.rs`): `rt_map_*` procedures (new, get, set, has, remove, clear, drop) and key hashing for text and integers.
- **Interpreter** (`src/interp.rs`): map values and the same operations.
- **Docs, examples, tests**: `docs/syntax.html`, `docs/native.html`, `docs/examples.html`, `README.md`, `examples/maps.br`, new `tests/maps.rs`.
- **Assumptions** (no clarification needed):
  - Keys are `text` or integers only; records, enums and floats as keys are rejected with a clear message.
  - Maps are shared references like lists: assigning a map to another variable does not copy it.
  - Iteration is in insertion order; removing a key and adding it again moves it to the end.
  - Changing a map while iterating over it is not supported (the result is unspecified but memory-safe).
  - Not included: sets, optional results for missing keys, record/enum keys, sorted maps.
