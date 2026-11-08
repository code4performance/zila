## MODIFIED Requirements

### Requirement: Types as compile-time values
The built-in type `Type` SHALL exist only at compile time. A `Type` value SHALL answer `name()` (the declared name, with type arguments for instances), `kind()` (one of `record`, `enum`, `alias`, `int`, `float`, `bool`, `text`, `list`, `map`, `set`, `optional`, `func`), `fields()` for records (each with `name`, `ty`, `attrs` and `line`), `variants()` for enums (each with `name`, `fields`, `attrs` and `line`), `element()`, `key()` and `value()` for containers, `inner()` for optionals and aliases, and `attrs()`; every one of these SHALL be callable from the packages that write generators. An attribute SHALL have a `name`, `args` (the literal arguments written in the source as text) and `quoted` (for each argument, whether it was written as a quoted text such as `"full_name"` or as a bare word such as `skip`). Using `Type` outside a comptime context SHALL be a compile error.

#### Scenario: Fields and attributes
- **WHEN** a generator receives `type User = record id: int, #[json("full_name")] name: text end`
- **THEN** `fields()` returns `id` of kind `int` with no attributes, and `name` of kind `text` with one attribute `json` whose first argument is `full_name` and whose `quoted[0]` is true

#### Scenario: Bare word and quoted text
- **WHEN** a generator receives a field with `#[json(skip)]` and another with `#[json("skip")]`
- **THEN** both attributes have the argument `skip`, and `quoted[0]` is false for the first and true for the second

#### Scenario: Enum variants
- **WHEN** a generator receives `type Shape = enum Circle(r: float), Dot end`
- **THEN** `variants()` returns `Circle` with one field `r` and `Dot` with none

#### Scenario: Type outside comptime
- **WHEN** a runtime function declares `let t: Type = ...`
- **THEN** compilation fails with an error that `Type` exists only at compile time

### Requirement: Types as arguments
A function declared `#[generated(gen)] func name(comptime t: Type, ...) -> ...` with no body SHALL be callable with a type name in the `comptime` position, or with the type as an explicit type argument (`name<Map<text, int>>(...)`, the only way to write a generic type). For each distinct type argument the compiler SHALL run `gen` with that type and compile the returned text as the body of a specialization; the result type MAY be `t`. When the generated function belongs to an imported package, its specialization SHALL be compiled in the module of the program that called it, so that the generated body sees that program's own types (private ones too), the types its signature names are written qualified by the library, and the calls to generated functions it makes (including to itself, for recursive types) are expanded in the next round like any others. A call whose `comptime` argument is not a type known at compile time SHALL be a compile error.

#### Scenario: One specialization per type
- **WHEN** a program calls `parse(Config, raw)` twice and `parse(User, raw)` once
- **THEN** the generator runs once for `Config` and once for `User`, and the first call returns a `Config`

#### Scenario: Runtime value as type
- **WHEN** a program calls `parse(kind_var, raw)` where `kind_var` is an ordinary variable
- **THEN** compilation fails saying the argument must be a type known at compile time

#### Scenario: A library function for the program's private types
- **WHEN** an imported package declares a `#[generated]` function whose generated body builds values of the reflected type and calls the same function for the types of its fields, and a program calls it with its own record type that has private fields and a nested record
- **THEN** the program compiles, both types get a specialization, and the calls inside the generated bodies reach them
