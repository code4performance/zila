## ADDED Requirements

### Requirement: The Eq, Ord and Hash interfaces
Without any `use`, the names `Eq` (`func eq(self, other: Self) -> bool`), `Ord` (`func compare(self, other: Self) -> int`) and `Hash` (`func hash(self) -> long`) SHALL be interfaces usable as bounds (`T: Ord`). `compare` SHALL return a negative number, zero or a positive number when `self` is before, equal to or after `other`. A bound of `Ord` or `Hash` SHALL also provide `eq`. A user declaration with one of these names in the program SHALL shadow the standard one in that package.

#### Scenario: Bound gives the methods
- **WHEN** `func largest<T: Ord>(xs: []T) -> T` calls `a.compare(b)` on two elements
- **THEN** it compiles, and `largest([3, 9, 4])` is 9 and `largest(["b", "a"])` is `b`

#### Scenario: Ord implies Eq
- **WHEN** a generic function bounded by `T: Ord` writes `a == b`
- **THEN** it compiles and is true exactly when `a.compare(b)` is 0

### Requirement: Derived methods for records and enums
For every record and enum, `eq`, `compare` and `hash` SHALL be derived when the type has no method of that name and every field (for an enum, every payload type of every variant) has the capability. A derived `eq` SHALL be true when all fields are equal; a derived `compare` SHALL compare fields in declaration order and return the first non-zero result; a derived `hash` SHALL combine the hashes of all fields so that equal values have equal hashes. Derived methods SHALL be ordinary methods that can be called directly.

#### Scenario: Record equality and order
- **WHEN** `type P = record x: int  y: int end` and `P { x: 1, y: 2 } == P { x: 1, y: 2 }`, and `P { x: 1, y: 9 } < P { x: 2, y: 0 }`
- **THEN** both comparisons are true, and `P { x: 1, y: 2 }.compare(P { x: 1, y: 3 })` is negative

#### Scenario: Equal values hash alike
- **WHEN** two records with equal fields have `hash` called
- **THEN** the results are equal, and in a program that hashes 1,000 records with distinct fields at least 990 hashes differ

#### Scenario: Nested types
- **WHEN** `Line` holds two `P` fields
- **THEN** `Line` has derived `eq`, `compare` and `hash` that use `P`'s

#### Scenario: Enums
- **WHEN** `type Shape = enum Dot  Circle(int)  Rect(int, int) end`
- **THEN** `Dot < Circle(1)`, `Circle(5) < Rect(0, 0)`, `Rect(1, 2) < Rect(1, 3)`, `Circle(2) == Circle(2)` and `Circle(2) != Rect(2, 2)`

#### Scenario: Recursive record
- **WHEN** a record `Tree` holds `left: Tree?`, `right: Tree?` and a value
- **THEN** `==` and `hash` on two equal trees work, finish, and `Tree` can be a map key

### Requirement: Overriding
A method named `eq`, `compare` or `hash` declared on a type with the interface's signature SHALL be used instead of the derived one, by direct calls, by operators, by maps and sets, by sorted collections and by generic code. A type that declares `eq` but not `hash` SHALL NOT be `Hash`.

#### Scenario: Custom equality
- **WHEN** `User` declares `func (a: User) eq(b: User) -> bool return a.id == b.id end` and `func (a: User) hash() -> long return long(a.id) end`
- **THEN** two users with the same `id` and different names are `==` and are one key in a `Map<User, int>`

#### Scenario: Custom order
- **WHEN** `Task` declares `compare` by descending priority
- **THEN** `list.sort(tasks)` puts the highest priority first and `a < b` follows it

#### Scenario: eq without hash
- **WHEN** a program uses `Map<User, int>` for a `User` that declares `eq` but not `hash`
- **THEN** compilation fails with an error saying `User` cannot be a map key because it defines eq but not hash

#### Scenario: Wrong signature
- **WHEN** a type declares `func (a: P) eq(b: int) -> bool` and is used as a key
- **THEN** compilation fails with an error that `eq` has the wrong signature for `Eq`

### Requirement: Rules for built-in types
`bool` and every integer type SHALL be `Eq`, `Ord` and `Hash` (false before true). `text` SHALL be `Eq`, `Ord` and `Hash` by content: two texts are equal when they have the same bytes, and `compare` orders by bytes. `float` and `f32` SHALL be `Eq` and `Ord` but not `Hash`; the operators `==`, `<` and the others on floats SHALL keep their IEEE meaning, while `compare` on floats SHALL be a total order in which NaN is after every number and equal to itself. An optional SHALL be `Eq`, `Ord` and `Hash` when its element is, with `nil` equal to `nil` and before any value. A list SHALL be `Eq`, `Ord` (lexicographic, a prefix first) and `Hash` when its element is. A map SHALL be `Eq` when its key and value are (same keys with equal values, in any insertion order) and `Hash` when its value is also `Hash`, and SHALL NOT be `Ord`.

#### Scenario: Text by content
- **WHEN** two texts built separately both hold `"abc"`
- **THEN** `==` is true, `compare` is 0 and their hashes are equal

#### Scenario: Floats are not hashable
- **WHEN** a program declares `Map<float, int>`, `set.Set<float>`, or a record with a `float` field used as a map key
- **THEN** compilation fails with an error that `float` is not Hash because NaN is not equal to itself

#### Scenario: Floats are comparable
- **WHEN** `func largest<T: Ord>` is called with floats and with a list that contains NaN
- **THEN** it compiles, returns the greatest value and does not loop or crash; `list.sort` of floats with a NaN puts the NaN last

#### Scenario: Containers
- **WHEN** a program compares `[1, 2] == [1, 2]`, `[1, 2] < [1, 3]`, `[1] < [1, 0]`, two optionals `3?` and `nil`, and the maps `{ "a": 1, "b": 2 }` and `{ "b": 2, "a": 1 }`
- **THEN** the results are true, true, true, `nil < 3` and the maps are equal

#### Scenario: Lists of records as keys
- **WHEN** a program uses `Map<[]int, text>` and `Map<P?, int>`
- **THEN** both compile and find a key by an equal list or optional

#### Scenario: Maps are not ordered
- **WHEN** a program writes `m1 < m2` for two maps, or declares `sorted.SortedSet<Map<text, int>>` through `sorted.set()`
- **THEN** compilation fails with an error that a map is not Ord

### Requirement: Identity for types held by reference
A value of a type held by reference (`new T`, `shared T`, or a record declared `#[shared]`) SHALL be `Eq` and `Hash` by identity (two names for one object are equal, two separate objects with equal fields are not) unless its type declares `eq` or `hash`, in which case the declared methods SHALL be used. Such a type SHALL NOT be `Ord` unless it declares `compare`.

#### Scenario: Identity
- **WHEN** two `#[shared]` records are built with the same field values
- **THEN** `a == b` is false, `a == a` is true, and as map keys they are two entries

#### Scenario: Eq on a shared type
- **WHEN** a `#[shared]` record declares `eq` and `hash` by its `id` field
- **THEN** two instances with the same `id` are `==` and are one map key

#### Scenario: No identity order
- **WHEN** a `#[shared]` record without `compare` is used as a `sorted.set()` element
- **THEN** compilation fails with an error that the type is not Ord

### Requirement: Operators on user types and bounded parameters
`==` and `!=` SHALL be defined for operands of the same type when the type is `Eq`, and call its `eq` (`!=` negates it); `<`, `<=`, `>` and `>=` SHALL be defined when the type is `Ord` and call its `compare`. For a type parameter, `==` and `!=` SHALL be defined when its bound is `Eq`, `Ord` or `Hash`, and the ordering operators when its bound is `Ord`. Using an operator on a type that lacks the capability SHALL be a compile error naming the type, the operator and the field or reason that blocks it. The operators on integers, text and floats SHALL behave as before.

#### Scenario: Generic maximum
- **WHEN** `func max<T: Ord>(a: T, b: T) -> T if a > b return a end return b end` is called with `3, 9`, with `"pear", "apple"` and with two `P` records
- **THEN** it returns the larger one in each case

#### Scenario: Missing capability
- **WHEN** a program writes `a < b` for a record that holds a map field
- **THEN** compilation fails with an error that `<` is not defined for the record because the field's map type is not Ord

#### Scenario: Bound too weak
- **WHEN** a generic function bounded by `T: Eq` writes `a < b`
- **THEN** compilation fails at the definition with an error that `<` needs `T: Ord`

#### Scenario: Not equal
- **WHEN** `a != b` is written for two records with different fields
- **THEN** it is true and `eq` was called once

### Requirement: Standard types and libraries follow the rules
Types declared by the standard library that are plain data (records and enums in `std/`) SHALL derive the interfaces under the same rules, and the standard library SHALL NOT define a method named `eq`, `compare` or `hash` with another meaning.

#### Scenario: Library records as keys
- **WHEN** a program uses a record declared by `use sorted` (an entry) as a set element
- **THEN** it compiles when its fields are `Hash`

### Requirement: Tests and documentation
`tests/derive.rs` SHALL cover every scenario of this capability, and the language documentation SHALL describe the three interfaces, derivation, overriding and the float and identity rules, with samples that compile.

#### Scenario: Documentation test
- **WHEN** the documentation test (`tests/docs.rs`) runs
- **THEN** every new code sample compiles and every new keyword or name used is documented
