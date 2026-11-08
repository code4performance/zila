## MODIFIED Requirements

### Requirement: Errors in generic code
The body of a generic function SHALL be checked once, as written, against what its bound (or lack of one) allows. Using a method or operator on a value of type parameter `T` that is not provided SHALL be a compile error at the generic definition naming `T`. The operators `==` and `!=` are provided by a bound of `Eq`, `Ord` or `Hash`, and `<`, `<=`, `>` and `>=` by a bound of `Ord`; no other operator is provided by any bound. Putting a `T` into text (`"{x}"`) is the one exception: it is checked for each instance, because Zila Lang has no built-in bound for printable types.

#### Scenario: Unbounded method call
- **WHEN** `func show<T>(x: T)` calls `x.to_text()`
- **THEN** compilation fails at the declaration with an error that `T` has no method `to_text`, suggesting a bound

#### Scenario: Unbounded operator
- **WHEN** `func add<T>(a: T, b: T) -> T` returns `a + b`
- **THEN** compilation fails at the declaration with an error that `+` is not available for `T`

#### Scenario: Unbounded comparison
- **WHEN** `func same<T>(a: T, b: T) -> bool` returns `a == b`
- **THEN** compilation fails at the declaration with an error that `==` is not available for `T`, suggesting the bound `T: Eq`

#### Scenario: Bounded comparison
- **WHEN** `func same<T: Eq>(a: T, b: T) -> bool` returns `a == b` and is called with integers, texts and records
- **THEN** it compiles and each call uses that type's equality
