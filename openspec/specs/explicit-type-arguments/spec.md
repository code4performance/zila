## Purpose

Generic functions can be called with type arguments written at the call site, which makes functions callable when inference has nothing to go on.

## Requirements


### Requirement: Explicit type arguments at a call
A call SHALL accept type arguments after the function name, `f<T, U>(args)` or `module.f<T>(args)`. The explicit types SHALL be bound to the type parameters in order, and the remaining parameters SHALL still be inferred. Giving more type arguments than the function has parameters SHALL be a compile-time error that names the function.

#### Scenario: Nothing to infer from
- **WHEN** `func empty<T>() -> []T` is called as `empty<int>()`
- **THEN** the call compiles and returns an empty list of `int`

#### Scenario: Several parameters
- **WHEN** `pair<int, text>(1, "x")` is called
- **THEN** both type parameters are bound from the call site

#### Scenario: Too many
- **WHEN** `id<int, int>(1)` is called and `id` has one type parameter
- **THEN** compilation fails with an error that mentions the type arguments

### Requirement: Comparisons are not calls
`<` SHALL start type arguments only when it follows a name without a space and the closing `>` is directly followed by `(`. An expression such as `a < b, b > (c)` SHALL remain two comparisons.

#### Scenario: Spaced operators
- **WHEN** `println(a < b, b > (c))` is compiled
- **THEN** it prints two booleans and no generic call is made
