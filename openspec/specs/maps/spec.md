## Purpose

Lets a program store values under keys and find them again quickly, so lookups by name (word counts, caches, configuration) do not need hand-written searches over lists.

## Requirements

### Requirement: The Map type
`Map<K, V>` SHALL be a type usable for variables, parameters, return values, record fields and list elements. `K` SHALL be any type that is `Hash` (and so `Eq`): `text`, integers, `bool`, records and enums whose parts are `Hash`, lists and optionals of such types, and types held by reference (by identity, or by their own `eq` and `hash`). `V` SHALL be any type the native compiler supports. A key type that is not `Hash` SHALL be rejected at compile time with a message naming the type and the reason (for example a `float` field, or an `eq` without a `hash`). Keys are found by `eq` and `hash`, so two keys that are `==` SHALL be one key.

#### Scenario: Valid map type
- **WHEN** a program declares `var ages: Map<text, int>`
- **THEN** it compiles

#### Scenario: Record key
- **WHEN** a program declares `Map<Point, text>` for `type Point = record x: int  y: int end`, stores `m[Point { x: 1, y: 2 }] = "a"` and reads `m[Point { x: 1, y: 2 }]`
- **THEN** it compiles and prints `a`, and storing again with an equal `Point` replaces the value so that `m.len` is 1

#### Scenario: Enum and list keys
- **WHEN** a program uses `Map<Shape, int>` for an enum and `Map<[]int, int>`
- **THEN** both compile and find entries by equal keys

#### Scenario: Unsupported key type
- **WHEN** a program declares `Map<Point, int>` where `Point` has a `float` field
- **THEN** compilation fails with an error saying that `Point` cannot be a map key because field `x: float` is not Hash

#### Scenario: Float key
- **WHEN** a program declares `Map<float, int>`
- **THEN** compilation fails with an error saying that `float` cannot be a map key because it is not Hash

#### Scenario: Many record keys
- **WHEN** a program inserts 100,000 distinct record keys, removes half, and reads the rest
- **THEN** every remaining key is found, `len` is 50,000 and the program finishes within a few seconds

#### Scenario: Keys are released
- **WHEN** a function that builds a map with text-holding record keys, replaces, removes and clears entries is called 5000 times
- **THEN** `live_objects()` afterwards equals the value before

### Requirement: Map literals
`{ k1: v1, k2: v2 }` SHALL build a map. The key and value types SHALL come from the expected type when there is one, otherwise from the first pair, and every other pair SHALL match them. `{}` SHALL require an expected type.

#### Scenario: Inferred types
- **WHEN** a program writes `let m = { "a": 1, "b": 2 }`
- **THEN** `m` is a `Map<text, int>` with two entries

#### Scenario: Mismatched pair
- **WHEN** a literal mixes `"a": 1` with `"b": "two"`
- **THEN** compilation fails with an error about the value type of the second pair

#### Scenario: Empty literal needs a type
- **WHEN** a program writes `let m = {}` with no annotation
- **THEN** compilation fails with an error asking for a type annotation

#### Scenario: Empty literal with a type
- **WHEN** a program writes `let m: Map<text, int> = {}`
- **THEN** it compiles and `m.len` is 0

### Requirement: Reading and writing by key
`m[key]` SHALL read the value stored under `key`, and `m[key] = value` SHALL store it, adding the key when absent and replacing the value when present. Reading a missing key SHALL abort the program with the message `map key not found` (use `has` or `get(key, default)` when the key may be absent).

#### Scenario: Insert and read
- **WHEN** a program sets `m["x"] = 5` and then prints `m["x"]`
- **THEN** it prints `5`

#### Scenario: Replace
- **WHEN** a program sets `m["x"] = 5` and then `m["x"] = 6`
- **THEN** `m["x"]` is `6` and `m.len` is 1

#### Scenario: Missing key
- **WHEN** a program reads `m["nope"]` and the key was never stored
- **THEN** the program aborts with `abort: <file>.zila:<line>: map key not found` and exits with 101

### Requirement: Map methods
A map SHALL provide the methods `has(key)` (bool), `get(key, default)` (the stored value or `default`), `remove(key)` (removes the key if present, otherwise does nothing) and `clear()` (removes every entry), and a `len` property (the number of entries, as for lists and text).

#### Scenario: has and get
- **WHEN** `m` holds `"a": 1`
- **THEN** `m.has("a")` is true, `m.has("b")` is false, `m.get("b", 0)` is 0 and `m.get("a", 0)` is 1

#### Scenario: remove
- **WHEN** a program removes `"a"` from a map holding `"a"` and `"b"`
- **THEN** `m.has("a")` is false and `m.len` is 1

#### Scenario: Removing a missing key
- **WHEN** a program removes a key that is not in the map
- **THEN** nothing happens and no error is raised

#### Scenario: clear
- **WHEN** a program calls `m.clear()` on a map with three entries
- **THEN** `m.len` is 0 and keys can be added again

### Requirement: Iteration in insertion order
`for key, value in m` SHALL visit every entry once, in the order the keys were first inserted; `for key in m` SHALL visit the keys in the same order. Replacing a value SHALL keep the key's position; removing a key and adding it again SHALL move it to the end.

#### Scenario: Order of insertion
- **WHEN** keys `"b"`, `"a"`, `"c"` are inserted in that order and a loop prints each key
- **THEN** the output is `b`, `a`, `c`

#### Scenario: Re-added key moves to the end
- **WHEN** `"b"` is removed from the map above and added again
- **THEN** a loop prints `a`, `c`, `b`

### Requirement: Many entries
A map SHALL keep working correctly as it grows, so that every inserted key is still found after thousands of insertions and removals.

#### Scenario: Ten thousand keys
- **WHEN** a program inserts the integer keys 0 to 9999, removes the even ones, and reads the rest
- **THEN** every odd key is found with its value, every even key is absent and `len` is 5000

### Requirement: No memory leaks
Maps, and the keys and values they own, SHALL be released when no longer used: when the last reference to the map goes away, when a key is removed, when a value is replaced and when the map is cleared.

#### Scenario: Many maps
- **WHEN** a function that builds a map of texts and records and sets, replaces and removes entries is called 5000 times
- **THEN** the number of live heap objects afterwards equals the number before

### Requirement: Maps have value semantics
Assigning a map to another variable or passing it to a function SHALL give the new holder an independent value without copying the entries: the entries are copied by the first write to a map that another holder still has (see `copy-on-write`). A change made through one name SHALL NOT be visible through the others, except through a `ref` parameter or receiver, or when the map is held by a value created with `new` or `shared`.

#### Scenario: Not changed through a parameter
- **WHEN** a function receives a map (without `ref`) and sets a key, and the caller then reads that key
- **THEN** the caller does not see the new key

#### Scenario: Changed through a parameter
- **WHEN** a function receives `ref m: Map<text, int>` and sets a key, and the caller then reads that key
- **THEN** the caller sees the new value

#### Scenario: Copy is independent
- **WHEN** a program does `var b = a` for a map `a` and sets `b["x"] = 1`
- **THEN** `a.has("x")` is false and `b["x"]` is 1
