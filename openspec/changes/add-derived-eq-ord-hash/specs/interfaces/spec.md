## MODIFIED Requirements

### Requirement: Structural satisfaction
A type SHALL satisfy an interface when, for every method of the interface, it has a method of the same name whose parameter types (after the receiver) and result type are identical. In an interface method signature the name `Self` SHALL stand for the type being checked, so `func eq(self, other: Self) -> bool` is satisfied by `func (p: Point) eq(o: Point) -> bool`; `Self` SHALL be rejected outside interface method signatures. A method the compiler derives for the standard interfaces (see `standard-interfaces`) SHALL count as a method of the type. No declaration linking the type to the interface SHALL be needed, and extra methods SHALL NOT matter.

#### Scenario: Satisfied without declaration
- **WHEN** record `Point` has `func (p: Point) to_text() -> text` and is passed to `show<T: Printable>`
- **THEN** it compiles

#### Scenario: Missing method
- **WHEN** record `Dog` has no `to_text` method and is passed to `show<T: Printable>`
- **THEN** compilation fails with an error that `Dog` does not satisfy `Printable` because it has no method `to_text`

#### Scenario: Mismatching signature
- **WHEN** `Cat` has `func (c: Cat) to_text() -> int` and is passed to `show<T: Printable>`
- **THEN** compilation fails with an error saying `to_text` returns `int` where `Printable` needs `text`

#### Scenario: Self in a user interface
- **WHEN** a program declares `type Shape = interface func bigger(self, other: Self) -> bool end` and `Square` has `func (a: Square) bigger(b: Square) -> bool`
- **THEN** `Square` satisfies `Shape`, and a type whose `bigger` takes an `int` does not

#### Scenario: Self outside an interface
- **WHEN** a program writes `Self` as a parameter type of an ordinary function
- **THEN** compilation fails with an error that `Self` is only valid inside an interface

#### Scenario: Derived method satisfies
- **WHEN** record `Point` declares no methods and is passed to `largest<T: Ord>`
- **THEN** it compiles, because `compare` is derived for it
