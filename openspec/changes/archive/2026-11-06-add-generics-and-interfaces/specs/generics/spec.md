## Purpose

Lets a program write one function or record for many types, with the compiler checking and generating the code for each type actually used.

## ADDED Requirements

### Requirement: Generic functions
A function declared with type parameters (`func first<T>(items: []T) -> T`) SHALL compile, and every distinct list of type arguments used by a call SHALL behave as if the function had been written for those types.

#### Scenario: One function, two types
- **WHEN** a program calls `first([1, 2, 3])` and `first(["a", "b"])`
- **THEN** it compiles and the calls return `1` and `"a"`

#### Scenario: Unused generic function
- **WHEN** a generic function is never called
- **THEN** the program compiles and runs as if it were absent

### Requirement: Type argument inference
Type arguments SHALL be inferred from the argument types of a call, and, for a parameter that appears only in the result, from the expected type of the call. A call that leaves a type parameter undetermined SHALL be a compile error naming it. A call whose arguments imply two different types for one parameter SHALL be a compile error naming both.

#### Scenario: Inferred from arguments
- **WHEN** `func pair<A, B>(a: A, b: B) -> text` is called as `pair(1, "x")`
- **THEN** `A` is `int` and `B` is `text`

#### Scenario: Inferred from the expected type
- **WHEN** `func empty<T>() -> []T` is called as `let xs: []int = empty()`
- **THEN** `T` is `int`

#### Scenario: Undetermined parameter
- **WHEN** `empty()` is called with no expected type
- **THEN** compilation fails with an error that the type of `T` cannot be inferred

#### Scenario: Conflicting arguments
- **WHEN** `func same<T>(a: T, b: T)` is called as `same(1, "x")`
- **THEN** compilation fails with an error saying `T` was both `int` and `text`

### Requirement: Generic records
A record declared with type parameters (`type Stack<T> = record items: []T end`) SHALL be usable as `Stack<int>` wherever a type is allowed, and a record literal SHALL infer its arguments from the expected type. Different type arguments SHALL give different, incompatible types.

#### Scenario: Instances differ
- **WHEN** a program passes a `Stack<text>` to a function expecting `Stack<int>`
- **THEN** compilation fails with an error that names both types (`Stack<i64>` expected, `Stack<text>` found)

#### Scenario: Literal with expected type
- **WHEN** a program writes `let s: Stack<int> = Stack { items: [1, 2] }`
- **THEN** it compiles and `s.items.len` is 2

#### Scenario: Wrong number of arguments
- **WHEN** a program writes `Stack<int, text>`
- **THEN** compilation fails with an error that `Stack` takes one type argument

### Requirement: Generic methods
A method whose receiver type has type parameters (`func (s: Stack<T>) push(x: T)`) SHALL be available on every instance of that record, with `T` bound to the instance's argument.

#### Scenario: Push and pop
- **WHEN** a program defines `push` and `pop` on `Stack<T>` and uses them on `Stack<int>` and `Stack<text>`
- **THEN** each stack stores and returns values of its own element type

### Requirement: Ownership
Instances SHALL follow the same reference-counting rules as hand-written code for the same types: no leaks and no double releases for texts, lists, records and maps stored in or returned from generic code.

#### Scenario: No leaks
- **WHEN** a generic function builds and returns lists of texts 1000 times
- **THEN** `live_objects()` does not grow between iterations

### Requirement: Errors in generic code
The body of a generic function SHALL be checked once, as written, against what its bound (or lack of one) allows. Using a method or operator on a value of type parameter `T` that is not provided SHALL be a compile error at the generic definition naming `T`. Putting a `T` into text (`"{x}"`) is the one exception: it is checked for each instance, because Brasa has no built-in bound for printable types.

#### Scenario: Unbounded method call
- **WHEN** `func show<T>(x: T)` calls `x.to_text()`
- **THEN** compilation fails at the declaration with an error that `T` has no method `to_text`, suggesting a bound

#### Scenario: Unbounded operator
- **WHEN** `func add<T>(a: T, b: T) -> T` returns `a + b`
- **THEN** compilation fails at the declaration with an error that `+` is not available for `T`

### Requirement: Unsupported forms
Generic enums, generic type aliases, async generic functions, generic closures and generic `extern` functions SHALL be rejected with a message that names the form.

#### Scenario: Async generic
- **WHEN** a program declares `async func f<T>(x: T) -> T`
- **THEN** compilation fails with an error that async generic functions are not supported

### Requirement: Interpreter
`--run` SHALL run programs that declare and use generic functions and records, with the same output as the native compiler.

#### Scenario: Same output
- **WHEN** `examples/generics.br` runs natively and with `--run`
- **THEN** the output is identical
