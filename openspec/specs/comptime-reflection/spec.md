## Purpose

Lets libraries extend the language without compiler changes: a compile-time function receives a description of a type and generates source code for it, and a library can define its own block syntax. Comptime code is a small, checked, deterministic subset of Brasa, compiled and run on the host at build time.

## Requirements

### Requirement: Types as compile-time values
The built-in type `Type` SHALL exist only at compile time. A `Type` value SHALL answer `name()` (the declared name, with type arguments for instances), `kind()` (one of `record`, `enum`, `alias`, `int`, `float`, `bool`, `text`, `list`, `map`, `set`, `optional`, `func`), `fields()` for records (each with `name`, `ty`, `attrs` and `line`), `variants()` for enums (each with `name`, `fields`, `attrs` and `line`), `element()`, `key()` and `value()` for containers, `inner()` for optionals and aliases, and `attrs()`. An attribute SHALL have a `name` and `args`, the literal arguments written in the source as text. Using `Type` outside a comptime context SHALL be a compile error.

#### Scenario: Fields and attributes
- **WHEN** a generator receives `type User = record id: int, #[json("full_name")] name: text end`
- **THEN** `fields()` returns `id` of kind `int` with no attributes, and `name` of kind `text` with one attribute `json` whose first argument is `full_name`

#### Scenario: Enum variants
- **WHEN** a generator receives `type Shape = enum Circle(r: float), Dot end`
- **THEN** `variants()` returns `Circle` with one field `r` and `Dot` with none

#### Scenario: Type outside comptime
- **WHEN** a runtime function declares `let t: Type = ...`
- **THEN** compilation fails with an error that `Type` exists only at compile time

### Requirement: Comptime functions generate source text
A function declared `comptime func` SHALL take `Type` and text parameters and return `text` containing Brasa source. It SHALL NOT be callable from runtime code. The compiler SHALL compile it for the host, run it at build time and parse the returned text; a parse or check error in the text SHALL be reported as described under diagnostics.

#### Scenario: A derive generates a method
- **WHEN** `#[derive(describe)] type Point = record x: int, y: int end` is built, and `describe` is a `#[derive] comptime func` returning the source of `func (p: Point) describe() -> text`
- **THEN** the program can call `p.describe()` and it behaves as the generated method defines

#### Scenario: Runtime call rejected
- **WHEN** a runtime function calls a `comptime func`
- **THEN** compilation fails with an error that comptime functions run only at build time

### Requirement: The comptime subset is enforced
A `comptime func`, and every function reachable from it, SHALL use only text, lists, maps, sets, records, enums, loops and ordinary calls into standard or package code. The compiler SHALL reject, naming the construct and the call chain from the comptime function: `extern` functions, `asm` blocks, `spawn`, `async`, `await` and channels, raw pointer and `unsafe` operations, reads and writes of global `var`s, and anything that performs I/O (files, sockets, the clock, the environment, the console).

#### Scenario: A generator that reads a file
- **WHEN** a comptime function calls a function that reaches `fs.read`
- **THEN** compilation fails naming `fs.read` and the chain `generator -> helper -> fs.read`

#### Scenario: A generator that spawns
- **WHEN** a comptime function contains `spawn`
- **THEN** compilation fails with an error that `spawn` is not allowed in comptime code

#### Scenario: Pure package code is allowed
- **WHEN** a comptime function calls helpers in an imported package that use only text and lists
- **THEN** compilation succeeds

### Requirement: Derive attributes
`#[derive(a, b)]` on a type declaration SHALL run, for each name, the `#[derive] comptime func` of that name found in an imported package (a qualified name such as `json.json` is allowed), passing the type, and SHALL add the returned declarations to the program. A name with no such function, or one that is ambiguous between two imported packages, SHALL be a compile error naming the type and the attribute. Output SHALL be able to declare functions, methods, constants and helper types, and SHALL NOT change the original declaration.

#### Scenario: Two derives
- **WHEN** a type has `#[derive(describe, size_hint)]` and both exist
- **THEN** both generated methods exist and the order of the names does not change behavior

#### Scenario: Unknown derive
- **WHEN** a type has `#[derive(nothing)]` and no imported package defines it
- **THEN** compilation fails at the attribute saying no derive named `nothing` was found in the imported packages

#### Scenario: Derive on an enum
- **WHEN** a derive is applied to an enum
- **THEN** the generator sees `kind() == enum` and its variants

### Requirement: Types as arguments
A function declared `#[generated(gen)] func name(comptime t: Type, ...) -> ...` with no body SHALL be callable with a type name in the `comptime` position. For each distinct type argument the compiler SHALL run `gen` with that type and compile the returned text as the body of a specialization; the result type MAY be `t`. A call whose `comptime` argument is not a type known at compile time SHALL be a compile error.

#### Scenario: One specialization per type
- **WHEN** a program calls `parse(Config, raw)` twice and `parse(User, raw)` once
- **THEN** the generator runs once for `Config` and once for `User`, and the first call returns a `Config`

#### Scenario: Runtime value as type
- **WHEN** a program calls `parse(kind_var, raw)` where `kind_var` is an ordinary variable
- **THEN** compilation fails saying the argument must be a type known at compile time

### Requirement: Generic block form
A line that consists of one identifier naming a `#[block] comptime func` of an imported package, followed by raw lines and a line that is only `end`, SHALL be a block. The raw text between the identifier line and that `end` SHALL be passed unchanged to the function, whose returned source replaces the block as an expression (or as statements when the block stands alone on a line). The grammar SHALL NOT give block syntax to any specific package; the special `html ... end` rule SHALL NOT exist.

#### Scenario: A library block
- **WHEN** a package defines `#[block] comptime func shout(raw: text) -> text` returning a text-literal expression of the upper-cased raw text, and a program writes `let s = shout`, a line `hello`, then `end`
- **THEN** `s` is `"HELLO"`

#### Scenario: Unknown block name
- **WHEN** a name alone on a line is not a block of any imported package
- **THEN** it is parsed as an ordinary expression, giving the usual error if it is undefined

#### Scenario: Opaque to tools
- **WHEN** the formatter or the language server reads a file containing a block whose content is not valid Brasa
- **THEN** the formatter leaves the content line for line as it was (re-indenting only the opening name and `end`) and the server reports no diagnostics for those lines

### Requirement: Hygiene
Names declared by generated declarations SHALL live in a synthetic module per generation, which sees the generator's package and the module of the reflected type but not the other names of the user's module, so generated helpers cannot capture or collide with user names. `reflect.fresh(base)` SHALL return a name unique within the build for local helpers. A generated method or top-level name that collides with an existing declaration SHALL be a compile error naming the generator and the declaration. Expressions produced by a block SHALL be resolved in the scope of the block's position, since they must refer to user variables.

#### Scenario: Helper cannot capture
- **WHEN** a derive emits a helper `func escape(s: text)` and the user's module also declares `escape`
- **THEN** both compile and each caller reaches its own

#### Scenario: Method collision
- **WHEN** a derive generates the method `describe` for `Point` and the user already declared `describe` for `Point`
- **THEN** compilation fails naming the derive and both declarations

### Requirement: Limits
Each comptime run SHALL be limited in wall-clock time (default 5 seconds, set with `--comptime-timeout <seconds>`) and in output size (1 MiB). Expansion of generated code that itself contains derives or blocks SHALL stop after 8 rounds. Exceeding a limit, a crash of the run or a non-zero exit of the run SHALL be a compile error naming the generator and where it was used, and SHALL never hang or crash the compiler.

#### Scenario: Endless generator
- **WHEN** a generator loops forever
- **THEN** the build fails after the time limit with an error naming the generator and the use site

#### Scenario: Runaway expansion
- **WHEN** a derive generates a type that derives the same derive again
- **THEN** the build fails after 8 rounds saying expansion did not settle

### Requirement: Execution model and cache
A comptime function SHALL be compiled with the normal compiler for the host target and run as a child process that inherits no handles except its input and output. Its result SHALL be a pure function of its code, its reflected inputs and the compiler version, and SHALL be cached on disk under a key hashed from those; a rebuild with nothing changed SHALL NOT run the generator again. `--no-comptime-cache` SHALL ignore the cache. No interpreter SHALL be used.

#### Scenario: Cached rebuild
- **WHEN** a program is built twice without changes
- **THEN** the second build does not start the generator process

#### Scenario: Changed generator
- **WHEN** the body of a generator, or of a function it calls, changes
- **THEN** the next build runs it again

#### Scenario: Changed type
- **WHEN** a field is added to a reflected record
- **THEN** the next build runs the generator again for that type

### Requirement: Cross-compilation
Comptime code SHALL always be built for and run on the host, whatever `--target` selects. The reflection API SHALL NOT expose target-dependent facts (sizes, offsets, alignment); `reflect.target()` SHALL return the name of the build target for generators that need to choose between behaviors.

#### Scenario: Linux target from Windows
- **WHEN** a program with a derive is built on Windows with `--target linux`
- **THEN** the generator runs as a Windows program and the output is a Linux executable with the generated methods

### Requirement: Diagnostics point at the user's code
An error raised by a generator (`reflect.fail(msg, at)`, where `at` is a type, field or variant) SHALL be reported at the line of that item in the user's code. A parse or check error in generated text SHALL be reported at the use site (the `#[derive]` attribute, the call or the block), naming the generator, and SHALL show the offending generated line and its text. `--comptime-dump` SHALL print all generated text.

#### Scenario: Generator refuses a field
- **WHEN** a derive calls `reflect.fail("fields of kind func cannot be encoded", field)` for a field declared on line 12
- **THEN** the error is reported at line 12 of the user's file with that message and the name of the derive

#### Scenario: Generated code does not compile
- **WHEN** a generator returns text with a type error on its third line
- **THEN** the error names the generator, the use site, and shows the third generated line
