## MODIFIED Requirements

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
