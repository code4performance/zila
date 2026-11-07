# ref-parameters Specification

## Purpose

Defines `ref` as a passing mode (a temporary borrow that cannot escape the call) and the compiler checks that enforce it.

## Requirements

### Requirement: ref is a passing mode, not a type
`ref` SHALL be allowed only before a function parameter, a method receiver, a lambda parameter and, repeated, before an argument at the call site. A `ref` parameter SHALL borrow the caller's value (a local, a field or a list element) for the duration of the call, and writes through it SHALL be visible to the caller. There SHALL be no `ref T` type: `ref` cannot appear in a field, element, return or variable type.

#### Scenario: Writes are visible
- **WHEN** `func fill(ref b: Buffer)` sets `b.n = 8` and the caller runs `fill(ref x)`
- **THEN** `x.n` is 8 afterwards

#### Scenario: Without ref the callee works on a copy
- **WHEN** a parameter has no `ref` and the callee changes a field
- **THEN** the caller's value is unchanged

### Requirement: A ref cannot escape
Outside an `unsafe` block, the compiler SHALL reject every use of a `ref` parameter, `ref` receiver or `ref` lambda parameter (a "ref binding") that could outlive the call. The rejected uses SHALL be: using it as the value of `let`, `var` or an assignment; storing it in a record field, record literal, list literal, map literal, list or map element, or a global; returning it; naming it inside any lambda, including one called immediately; passing it to `spawn`; passing it to an `async` function; passing it to a parameter that has no `ref`; and taking `&` of it. The error SHALL name the parameter, say that a `ref` cannot escape, and say to use `new T` for a value that must live longer.

#### Scenario: Returned
- **WHEN** `func id(ref r: R) -> R` contains `return r`
- **THEN** compilation fails with an error containing "cannot escape" and "use `new T`"

#### Scenario: Stored in a record, list, map or global
- **WHEN** a ref binding is assigned to a field (`h.last = r`), added to a list (`xs.add(r)`), put in a map (`m[k] = r`), put in a literal (`[r]`, `Holder{r: r}`) or assigned to a global
- **THEN** compilation fails with the cannot-escape error for each

#### Scenario: Assigned to another variable
- **WHEN** a body contains `let c = r`, `var c = r` or `c = r` with `r` a ref binding
- **THEN** compilation fails with the cannot-escape error

#### Scenario: Captured by a lambda
- **WHEN** a lambda body mentions a ref binding of the enclosing function, even if the lambda is called at once
- **THEN** compilation fails with the cannot-escape error

#### Scenario: Passed to spawn or an async call
- **WHEN** a body contains `spawn work(r)`, `spawn work(ref r)`, or a call of an `async` function with `r`
- **THEN** compilation fails with the cannot-escape error

#### Scenario: Address taken
- **WHEN** a body contains `&r` for a ref binding `r`
- **THEN** compilation fails with the cannot-escape error

#### Scenario: Passed by value
- **WHEN** a ref binding is passed as `f(r)` to a parameter without `ref`
- **THEN** compilation fails with the cannot-escape error

#### Scenario: Unsafe code is exempt
- **WHEN** the same use appears inside an `unsafe` block
- **THEN** it is not rejected by this rule

### Requirement: Borrowing uses stay allowed
A ref binding SHALL be usable as the base of a field access, an index, a method call or `.len`; as the target of a field or element assignment; as the iterated expression of `for`; and as a repeated `ref` argument to a function or method (`g(ref r)`), which passes the borrow further down. A ref binding of a scalar type SHALL additionally be usable as a value (a copy) and as the target of an assignment. A local variable or lambda parameter with the same name declared in an inner scope SHALL shadow the ref binding in that scope.

#### Scenario: Field and element access
- **WHEN** a body reads and writes `r.x` and `r.items[0]`, calls `r.items.add(1)` and runs `for v in r.items`
- **THEN** the program compiles and behaves as before

#### Scenario: Passed down
- **WHEN** `chan_send(ref ch, w)` calls `wake_one(ref ch)` with `ch` a ref parameter
- **THEN** the program compiles and `wake_one` sees the same value

#### Scenario: Scalar ref
- **WHEN** `func bump(ref v: int) -> int` runs `v += 10` and `return v`
- **THEN** it compiles, the caller's variable changes and the returned value is a copy

#### Scenario: Reading a field copies it
- **WHEN** `let n = r.name` and `return r.count` appear with `r` a ref binding
- **THEN** both compile

#### Scenario: Shadowing
- **WHEN** an inner `let r = Holder{}` or a lambda parameter named `r` shadows a ref binding `r`
- **THEN** uses of the shadowing name are not errors

### Requirement: ref receivers
A method declared with `(ref s: T)` SHALL treat `s` as a ref binding in its body. A method whose receiver has no `ref` SHALL work on a copy and SHALL NOT be restricted.

#### Scenario: Receiver mutates
- **WHEN** `func (ref s: Server) use(m: Middleware)` runs `s.chain.add(m)`
- **THEN** it compiles and the caller's server changes

#### Scenario: Receiver escapes
- **WHEN** such a method runs `return s` or `registry.add(s)`
- **THEN** compilation fails with the cannot-escape error

### Requirement: Async functions take no ref parameters
An `async` function or method SHALL NOT declare a `ref` parameter or receiver, since the task may outlive the caller. Declaring one SHALL be a compile-time error that names the function.

#### Scenario: Async with ref
- **WHEN** `async func g(ref r: R)` is declared
- **THEN** compilation fails with an error containing "cannot take `ref` parameters"
