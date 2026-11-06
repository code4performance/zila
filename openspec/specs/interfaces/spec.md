## Purpose

Describes what a type must be able to do, so generic code can rely on those abilities without knowing the concrete type.

## Requirements

### Requirement: Interface declarations
`type Name = interface ... end` SHALL declare a set of method signatures, each `func name(self[, params]) [-> result]`. Method names in one interface SHALL be unique.

#### Scenario: Declaration
- **WHEN** a program declares `Printable` with one method `to_text(self) -> text`
- **THEN** it compiles

#### Scenario: Duplicate method
- **WHEN** an interface declares `to_text` twice
- **THEN** compilation fails with an error naming the duplicate

### Requirement: Structural satisfaction
A type SHALL satisfy an interface when, for every method of the interface, it has a method of the same name whose parameter types (after the receiver) and result type are identical. No declaration linking the type to the interface SHALL be needed, and extra methods SHALL NOT matter.

#### Scenario: Satisfied without declaration
- **WHEN** record `Point` has `func (p: Point) to_text() -> text` and is passed to `show<T: Printable>`
- **THEN** it compiles

#### Scenario: Missing method
- **WHEN** record `Dog` has no `to_text` method and is passed to `show<T: Printable>`
- **THEN** compilation fails with an error that `Dog` does not satisfy `Printable` because it has no method `to_text`

#### Scenario: Mismatching signature
- **WHEN** `Cat` has `func (c: Cat) to_text() -> int` and is passed to `show<T: Printable>`
- **THEN** compilation fails with an error saying `to_text` returns `int` where `Printable` needs `text`

### Requirement: Bounds
A type parameter MAY have one interface bound (`T: Printable`, or `T: pkg.Printable`). Every type argument SHALL satisfy the bound, and the generic body MAY call the bound's methods on values of type `T`. Calls SHALL dispatch statically to the instance's own method.

#### Scenario: Calling a bound method
- **WHEN** `show<T: Printable>(x: T)` calls `x.to_text()` and is used with two different record types
- **THEN** each call runs that record's own `to_text`

#### Scenario: Bound on a generic record
- **WHEN** `type Box<T: Printable>` has a method that calls `to_text` on its item
- **THEN** `Box<Point>` works and `Box<Dog>` is a compile error naming `Dog` and `Printable`

#### Scenario: Bound is not an interface
- **WHEN** a bound names a type that is not an interface
- **THEN** compilation fails with an error that the bound must be an interface

### Requirement: Interfaces only as bounds
An interface used as the type of a variable, parameter, field, element or result SHALL be rejected with a message saying interfaces can only bound type parameters and suggesting `<T: Name>`.

#### Scenario: Interface-typed parameter
- **WHEN** a program declares `func show(x: Printable)`
- **THEN** compilation fails with an error suggesting `func show<T: Printable>(x: T)`

### Requirement: Built-in types as arguments
A built-in type MAY be a type argument. It SHALL satisfy a bound when methods with a matching receiver exist for it, which Brasa code can declare (`func (n: int) to_text() -> text`).

#### Scenario: Method on a built-in type
- **WHEN** a program declares `func (n: int) to_text() -> text` and calls `show(42)` with `T: Printable`
- **THEN** it compiles and runs `to_text` on the integer
