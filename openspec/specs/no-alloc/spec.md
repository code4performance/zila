# no-alloc Specification

## Purpose

Lets a function promise that it never allocates, with the compiler checking the promise through the whole call graph and naming the chain when it fails.

## Requirements

### Requirement: The no_alloc attribute
A function declared with `#[no_alloc]` SHALL compile only if neither its body nor any function it calls allocates. The attribute SHALL be accepted on functions and methods of any module and SHALL be rejected on anything else with a message naming the declaration.

#### Scenario: A function that does not allocate
- **WHEN** a `#[no_alloc]` function only does arithmetic, reads and writes to a unique local and calls functions that do the same
- **THEN** the program compiles

#### Scenario: Attribute on a type
- **WHEN** `#[no_alloc]` is written before a `type` declaration
- **THEN** compilation fails saying the attribute only applies to functions

### Requirement: Allocating operations are rejected
Inside a `#[no_alloc]` function the compiler SHALL reject text interpolation, list, map and set literals with elements, `new`, closures that capture a variable, `spawn`, `channel`, `defer`, `c.alloc` and the creation of a future by calling an async function. A text literal without interpolation and a closure that captures nothing SHALL be accepted.

#### Scenario: Interpolation
- **WHEN** a `#[no_alloc]` function contains `"n = {n}"`
- **THEN** compilation fails with an error naming the function, the operation "text interpolation" and its line

#### Scenario: Constant text and non-capturing closure
- **WHEN** a `#[no_alloc]` function returns the literal `"ready"` and passes a closure that uses only its own parameters to a named function
- **THEN** the program compiles

#### Scenario: Capturing closure
- **WHEN** a `#[no_alloc]` function builds a closure that uses a local variable of the function
- **THEN** compilation fails naming "closure that captures `<variable>`"

### Requirement: Calls are analysed transitively
A call from a `#[no_alloc]` function to a function that allocates, directly or through other calls, SHALL be rejected, and the error SHALL show the chain of calls from the `#[no_alloc]` function to the first allocating operation. The callee SHALL NOT need the attribute: a function the analysis finds non-allocating is accepted. The standard library SHALL be analysed in the same way as user code, with no separate list of allocating functions. Recursive functions SHALL be handled.

#### Scenario: Chain in the message
- **WHEN** `#[no_alloc] func a()` calls `b`, `b` calls `c`, and `c` contains a list literal with elements
- **THEN** the error reads like `a -> b -> c -> list literal` with the file and line of the literal

#### Scenario: Callee without attribute that does not allocate
- **WHEN** a `#[no_alloc]` function calls an ordinary helper that only adds numbers
- **THEN** the program compiles

#### Scenario: Allocating library function
- **WHEN** a `#[no_alloc]` function calls a standard library function whose body creates a text or a list
- **THEN** compilation fails with a chain that ends inside that library function

#### Scenario: Recursion
- **WHEN** a `#[no_alloc]` function calls itself and nothing in the cycle allocates
- **THEN** the program compiles

### Requirement: Indirect calls are rejected
Inside a `#[no_alloc]` function, a call through a function value (a parameter, a field, a list element or a local holding a function) and a call to an interface method whose concrete type is not known at the call site SHALL be rejected, naming the callee expression. An interface method call on a receiver of a known concrete type SHALL be analysed as a direct call.

#### Scenario: Function value
- **WHEN** a `#[no_alloc]` function takes a parameter `cb` of function type and calls `cb(1)`
- **THEN** compilation fails naming "call through function value `cb`"

#### Scenario: Interface call
- **WHEN** a `#[no_alloc]` function calls `shape.area()` where `shape` has an interface type
- **THEN** compilation fails naming "interface call `area`"

### Requirement: Generic instantiations are analysed
A generic function called from a `#[no_alloc]` function SHALL be checked for the type arguments of that call. An instantiation that allocates SHALL be rejected with an error naming the instantiation, and one that does not SHALL be accepted.

#### Scenario: Two instantiations
- **WHEN** a generic `func first<T>(items: List<T>) -> T` is called from `#[no_alloc]` code with `List<i32>`, and a generic function that builds a text from a `T` is called with `i32`
- **THEN** the first call is accepted and the second is rejected naming `<i32>`

### Requirement: Writes that may copy are rejected
Inside a `#[no_alloc]` function, a write to a list, text or record that the compiler cannot show to be uniquely owned, and that the copy-on-write rules say may copy first, SHALL be rejected with a message that the write may copy. A write to a uniquely owned value SHALL be accepted.

#### Scenario: Shared value
- **WHEN** a `#[no_alloc]` function writes an element of a list parameter that other holders may share
- **THEN** compilation fails saying the write may copy

#### Scenario: Unique local
- **WHEN** a `#[no_alloc]` function writes an element of a fixed array it declared itself and never shared
- **THEN** the program compiles

### Requirement: Externs and assembly are trusted
An `extern` function SHALL be treated as non-allocating unless it is declared `#[allocates]`, in which case calling it from `#[no_alloc]` code SHALL be rejected. The contents of an `asm` block SHALL NOT be analysed.

#### Scenario: Operating system call
- **WHEN** a `#[no_alloc]` function calls an `extern` function with no `#[allocates]`
- **THEN** the program compiles

#### Scenario: Runtime hook that allocates
- **WHEN** a `#[no_alloc]` function calls an `extern` declared `#[allocates]`
- **THEN** compilation fails naming the extern

### Requirement: Standard library functions that cannot allocate are marked
Functions of the standard library (`std/`) that never allocate, such as the length, index and arithmetic helpers, SHALL carry `#[no_alloc]` so the compiler keeps them that way, and a change that makes one of them allocate SHALL fail the build of the library.

#### Scenario: Marked function allocates
- **WHEN** a `#[no_alloc]` function in `std/` is edited to build a list
- **THEN** compiling any program that imports that module fails with the chain
