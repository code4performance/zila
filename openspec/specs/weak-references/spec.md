## Purpose

Shared, reference-counted objects (`new`, `Rc<T>`) and weak references (`Weak<T>`, `weak`, `upgrade`), so that parent links, observers and graphs can be built without leaking reference cycles.

## Requirements

### Requirement: Shared objects with new
`new T{ ... }`, where `T` is a record type (possibly generic), SHALL allocate one heap object holding a `T`, with a strong count of one, and have type `Rc<T>`. `new T` without a record literal, and `new` applied to a type that is not a record, SHALL be a compile error that says a record literal is required. Records SHALL remain value types: only an `Rc<T>` is shared.

#### Scenario: Create and read
- **WHEN** a program declares `type Node = record value: int end` and runs `let n = new Node{ value: 7 }` then `print("{n.value}")`
- **THEN** it prints `7`

#### Scenario: Missing literal
- **WHEN** a program contains `let n = new Node`
- **THEN** compilation fails with an error saying `new` needs a record literal such as `new Node{ ... }`

#### Scenario: Not a record
- **WHEN** a program contains `new int`
- **THEN** compilation fails with an error saying `new` needs a record literal

### Requirement: Sharing and mutation through Rc
Assigning, passing, returning, storing in a field, list or map, and capturing an `Rc<T>` SHALL NOT copy the object: every copy of the reference SHALL refer to the same object and add one to its strong count. Fields SHALL be readable and assignable through an `Rc<T>` (`n.value = 8`), methods of `T` SHALL be callable on it (receivers declared `ref` see the shared object), and a change made through one reference SHALL be visible through all.

#### Scenario: Two names, one object
- **WHEN** `let a = new Node{ value: 1 }`, `let b = a` and `b.value = 5` run
- **THEN** `a.value` is `5`

#### Scenario: Passed to a function
- **WHEN** a function takes `n: Rc<Node>` and sets `n.value = 9`
- **THEN** the caller's object has `value` 9 after the call

#### Scenario: Reference counted
- **WHEN** an `Rc<Node>` is created, copied into a list, and both the variable and the list go out of scope
- **THEN** `live_objects()` returns to its earlier value

### Requirement: Weak references
`Weak<T>` SHALL be a generic type. `weak(x)` with `x: Rc<T>` SHALL return a `Weak<T>` that does not add to the strong count and does not keep the object alive. `w.upgrade()` SHALL return `Rc<T>?`: a new strong reference while the object is alive, and `nil` once the strong count has reached zero. `weak(x)` called twice for one object SHALL give references to the same object. A `Weak<T>` MAY be `nil`, and `upgrade()` on `nil` SHALL return `nil`. `weak` applied to anything other than an `Rc<T>` SHALL be a compile error.

#### Scenario: Upgrade while alive
- **WHEN** `let n = new Node{ value: 3 }` and `let w = weak(n)` run and then `w.upgrade()` is read
- **THEN** it is not `nil` and its `value` is `3`

#### Scenario: Upgrade after the last strong reference is gone
- **WHEN** the only `Rc<Node>` of an object goes out of scope while a `Weak<Node>` to it is still held, and `upgrade()` is called
- **THEN** it returns `nil`

#### Scenario: Weak does not keep the object alive
- **WHEN** an object is created in an inner scope, `weak(n)` is stored in an outer variable, and the inner scope ends
- **THEN** the object has been freed and its `drop` ran; only the small weak handle remains counted by `live_objects()`

#### Scenario: Upgrade of nil
- **WHEN** a `Weak<Node>` is `nil` and `upgrade()` is called
- **THEN** the result is `nil`

#### Scenario: weak of a non-Rc
- **WHEN** a program calls `weak(5)` or `weak` on a plain record value
- **THEN** compilation fails with an error saying `weak` takes an `Rc<T>`

### Requirement: Weak handle lifetime
A weak reference SHALL be a counted heap object (one per shared object, shared by all `Weak<T>` made from it), so that it stays valid after the target is freed. It SHALL be freed when the last `Weak<T>` to it is gone and the target is gone, and SHALL be counted by `live_objects()` while it exists. The target's own storage SHALL be freed when its strong count reaches zero, regardless of how many weak references remain.

#### Scenario: Handle outlives the object
- **WHEN** a `Weak<Node>` is kept after the object has been freed
- **THEN** `upgrade()` returns `nil` and no memory error occurs

#### Scenario: Handle released with the last weak reference
- **WHEN** an object and every `Weak<T>` made from it have gone out of scope
- **THEN** `live_objects()` is back to its value before the object was created

### Requirement: drop and the strong count
When the strong count of an `Rc<T>` reaches zero, the weak handle SHALL first be cleared (so `upgrade()` returns `nil` from then on, including inside `drop`), then `T`'s `drop` method (if any) SHALL run exactly once with the object still intact, then the fields SHALL be released. Releasing a weak reference SHALL never run `drop`.

#### Scenario: drop runs once at zero
- **WHEN** an `Rc<Res>` whose `Res` has a `drop` that prints is copied three times and every copy goes out of scope
- **THEN** `drop` ran exactly once, after the last copy went away

#### Scenario: Upgrade inside drop
- **WHEN** a `drop` method calls `upgrade()` on a weak reference to its own object
- **THEN** the result is `nil`

#### Scenario: Weak release does not drop
- **WHEN** a `Weak<Res>` goes out of scope while a strong reference remains
- **THEN** `drop` does not run

### Requirement: Parent and child cycles
A field of type `Weak<T>` SHALL allow a child to refer to its parent. A parent that holds its children as `[]Rc<T>` with children holding `parent: Weak<T>` SHALL be freed completely when the last outside strong reference to the parent goes away. A cycle made only of `Rc<T>` references SHALL still not be freed; the language does not detect it.

#### Scenario: Tree with weak parents is freed
- **WHEN** a program records `let before = live_objects()`, builds a parent with ten children whose `parent` field is `weak(parent)`, reads `child.parent.upgrade()` successfully for each child, and lets the parent go out of scope
- **THEN** `live_objects() - before` is `0`

#### Scenario: Strong cycle still leaks
- **WHEN** two `Rc<Node>` objects hold each other in `Rc<Node>` fields and all outside references go away
- **THEN** `live_objects() - before` is `2`

#### Scenario: Children outlive the parent
- **WHEN** only the children are kept after the parent's last strong reference is dropped
- **THEN** each child's `parent.upgrade()` returns `nil`

### Requirement: Assignment and comparison
Assigning an `Rc<T>` or a `Weak<T>` SHALL copy the reference, never the contents, and SHALL release the value that was in the target. `==` and `!=` between two `Rc<T>` SHALL compare identity (the same object), not field values; likewise between two `Weak<T>` (the same object, even when it is gone); comparing an `Rc<T>` or `Weak<T>` with `nil` SHALL test for `nil`. Comparing an `Rc<T>` with a `Weak<T>`, or either with a plain `T`, SHALL be a compile error. Neither type SHALL be accepted as a map key.

#### Scenario: Identity, not contents
- **WHEN** `a = new Node{ value: 1 }` and `b = new Node{ value: 1 }`
- **THEN** `a == b` is false and `a == a` is true

#### Scenario: Weak to the same object
- **WHEN** `weak(a) == weak(a)` is evaluated
- **THEN** it is true

#### Scenario: Mixed comparison rejected
- **WHEN** a program compares an `Rc<Node>` with a `Weak<Node>`
- **THEN** compilation fails with an error naming both types

### Requirement: Closure capture
A closure that uses an `Rc<T>` SHALL hold a strong reference for its own lifetime (captures are by value). A closure that uses a `Weak<T>` SHALL hold a weak reference, so capturing `weak(node)` does not keep the node alive and breaks the cycle between a node and a callback stored in it.

#### Scenario: Weak capture breaks a cycle
- **WHEN** a node stores a callback that captured `weak(node)`, and the last outside reference to the node goes away
- **THEN** `live_objects()` returns to its earlier value and the callback is freed with the node

#### Scenario: Strong capture keeps the object
- **WHEN** a closure captures an `Rc<Node>` and every other reference to the node goes away
- **THEN** the node stays alive until the closure is freed
