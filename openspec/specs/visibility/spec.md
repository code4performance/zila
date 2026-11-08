## Purpose

Private-by-default declarations and the `pub` modifier: package scope, qualified `use`, record fields, construction across packages, public signatures, generics, interfaces and compiler-generated references.

## Requirements

### Requirement: Private by default
A top-level function, type, `let` / `var`, method, interface or record field SHALL be visible only inside the `package` that declares it unless it is written with `pub`. Every file of the same package SHALL see all of the package's names, `pub` or not. `pub` SHALL be a reserved word.

#### Scenario: Private helper in the same package
- **WHEN** package `text` has a non-`pub` function `index_from` and another function of `text` calls it
- **THEN** the program compiles

#### Scenario: Private name from another package
- **WHEN** a program that says `use text` calls `text.index_from(...)` and `index_from` is not `pub`
- **THEN** compilation fails with ``'index_from' is private to package 'text'; mark it `pub` to use it from another package`` and the line of the call

#### Scenario: Undefined is not private
- **WHEN** a program calls `text.no_such(...)`
- **THEN** the error says the module has no such function, not that it is private

#### Scenario: pub is reserved
- **WHEN** a program declares a variable named `pub`
- **THEN** it fails to parse

### Requirement: The pub modifier
`pub` SHALL be accepted before `func` (after attributes, before `async`), before a method declaration, before `type` (records, enums, interfaces, aliases), before top-level `let` and `var`, before `extern ... func`, and before a record field name. `pub` on an enum variant, an interface method signature, a parameter, `embed`, `package` or `use` SHALL be a parse error. A `pub` enum SHALL export all its variants.

#### Scenario: Exported declarations
- **WHEN** a library declares `pub func`, `pub async func`, `pub type`, `pub let`, `pub var`, `pub extern func`, and a method `pub func (s: Stack) push(x: int)`
- **THEN** a program can use each of them through the package name

#### Scenario: Variants follow the type
- **WHEN** `pub type Color = enum Red, Green end` is declared in `shapes`
- **THEN** `shapes.Color.Red` is usable from another package, and a non-`pub` enum's variants are not

#### Scenario: Misplaced pub
- **WHEN** `pub embed "x"` or a `pub` variant is written
- **THEN** the parse error names what `pub` cannot be applied to

### Requirement: Use exposes public names qualified
`use pkg` SHALL make only the `pub` names of `pkg` reachable, and only as `pkg.name`. It SHALL NOT make the packages that `pkg` uses reachable.

#### Scenario: Qualified access only
- **WHEN** a program says `use text`
- **THEN** `text.trim(s)` works with `trim` public, and a bare `trim(s)` is undefined

#### Scenario: Transitive use is not exposed
- **WHEN** `text` says `use float` and a program says only `use text`
- **THEN** the program cannot write `float.something`

### Requirement: Record fields
Record fields SHALL be private unless written `pub`. Reading, assigning or taking a field of another package's record SHALL be a compile error unless the field is `pub`, with a message naming the field and its package. Copying, passing, returning or storing the whole value SHALL always be allowed.

#### Scenario: Private field read
- **WHEN** a program reads `buf.len` and `len` is a private field of `mem.Buffer`
- **THEN** compilation fails with ``field 'len' of 'mem.Buffer' is private to package 'mem'``

#### Scenario: Whole value passes freely
- **WHEN** a program stores a `mem.Buffer` returned by a `pub` function in a list and passes it to another `pub` function
- **THEN** it compiles

### Requirement: Constructing records across packages
A record literal or `new` of a type declared in another package SHALL be a compile error if the type has any private field; if every field is `pub` the literal SHALL be allowed and SHALL name only existing fields.

#### Scenario: Private fields block the literal
- **WHEN** a program writes `text.Builder { n: 0 }` and `Builder` has a private field
- **THEN** compilation fails with ``cannot create 'text.Builder' here: it has private fields; use a function from package 'text'``

#### Scenario: Constructor function
- **WHEN** the library exports `pub func new_builder() -> Builder`
- **THEN** the program creates values by calling it

#### Scenario: All-public record
- **WHEN** every field of `geo.Point` is `pub`
- **THEN** `geo.Point { x: 1, y: 2 }` compiles in another package

### Requirement: No private types in public signatures
A `pub` function, method, global, record field or alias SHALL NOT name a private type of its own package in its signature or declared type.

#### Scenario: Leaked type
- **WHEN** `pub func open() -> Handle` is declared and `Handle` is not `pub`
- **THEN** compilation fails with ``pub func 'open' exposes private type 'Handle'; mark 'Handle' pub or make 'open' private``

### Requirement: Generics keep the defining package's access
A generic function or type SHALL, when instantiated from another package, access the private names of the package that declared it, and SHALL NOT give the instantiating package access to them. Type arguments SHALL be resolved in the instantiating package.

#### Scenario: Generic uses a private helper
- **WHEN** `list` has a `pub` generic function that calls a private helper of `list`, and a program calls it with its own record type
- **THEN** the program compiles and runs

#### Scenario: Caller's private type as argument
- **WHEN** a program passes a value of its own non-`pub` type to a `pub` generic of another package
- **THEN** it compiles

#### Scenario: No leakage to the caller
- **WHEN** that program then calls the generic's private helper directly
- **THEN** compilation fails with the private-name error

### Requirement: Interfaces and visibility
A type SHALL satisfy an interface only through methods visible from the interface's package (`pub` methods, or methods of the interface's own package).

#### Scenario: Public method satisfies
- **WHEN** `pub type Printable = interface func show(self) -> text end` is in `fmtlib` and a program type has `pub func (p P) show()`
- **THEN** a generic bounded by `T: fmtlib.Printable` accepts `P`

#### Scenario: Private method does not
- **WHEN** the program type's `show` is not `pub`
- **THEN** the instantiation fails, saying `show` is private to the program's package

### Requirement: Compiler-generated references need no pub
Calls inserted by the compiler (task and future runtime for `spawn` / `await` / channels, floating-point helpers, `format.fmt` renaming, intrinsics, implicit `drop`, test functions run by `--test`, `main`, `#[export]` functions) SHALL work without `pub`.

#### Scenario: Private main and test
- **WHEN** a program has non-`pub` `main` and `test_` functions
- **THEN** it runs and `--test` finds the tests

### Requirement: Tooling understands pub
The formatter SHALL format `pub` declarations idempotently, the language server SHALL report no syntax error for them, and the VS Code grammar SHALL highlight `pub` as a declaration keyword.

#### Scenario: Format and parse
- **WHEN** a file with `pub func`, `pub type` and `pub` fields is formatted twice
- **THEN** the second run changes nothing and the language server reports no diagnostics
