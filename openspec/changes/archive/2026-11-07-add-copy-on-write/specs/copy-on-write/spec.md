## ADDED Requirements

### Requirement: Heap values inside values keep value semantics
Assigning, passing, returning, capturing in a closure, or storing a record, list, map or mutable text buffer in another record field, list element or map value SHALL give the new holder a logically independent value: a later write through one holder SHALL NOT be visible through any other holder. This SHALL hold for records that contain lists, maps and buffers, at any depth. Only values created with `new T` or `shared T` SHALL share mutable state between holders.

#### Scenario: Record with a list
- **WHEN** a program does `var b = a` where `a: Bag` has a list field `items`, then `b.items.push(3)`
- **THEN** `b.items.len` is one more than before and `a.items.len` is unchanged

#### Scenario: Plain list
- **WHEN** a program does `var ys = xs` and then `ys[0] = 99`
- **THEN** `xs[0]` still has its old value

#### Scenario: Passed to a function
- **WHEN** a function receives a list parameter (without `ref`) and appends to it, and the caller then reads the list
- **THEN** the caller's list has its old length

#### Scenario: Map inside a record inside a list
- **WHEN** a program copies a list of records that hold maps and writes `copy[0].m["k"] = 1`
- **THEN** `original[0].m.has("k")` is unchanged

#### Scenario: Returned value
- **WHEN** a function returns a list field of a record and the caller appends to the result
- **THEN** the record's field is unchanged

#### Scenario: Shared records are exempt
- **WHEN** a record is created with `new T` (or `shared T`) and assigned to a second name, and one name appends to its list field
- **THEN** the other name sees the change

### Requirement: A copy does not copy elements
Copying a value that holds a list, map or buffer SHALL NOT copy the elements; the copy and the original SHALL refer to the same storage until one of them is written. A write to a value no one else holds SHALL be done in place with no allocation.

#### Scenario: No copy until a write
- **WHEN** a program records `live_objects()`, assigns a list of 1,000 integers to a second name, and reads `live_objects()` again without writing
- **THEN** the count has not changed

#### Scenario: Unique writes do not copy
- **WHEN** a program appends 10,000 integers to a list that no other name holds and reads `live_objects()` before and after
- **THEN** the count has not changed

#### Scenario: The first write to a shared value copies once
- **WHEN** a list held by two names is written through one of them three times
- **THEN** `live_objects()` grew by one after the first write and did not grow after the others, and the other name's list is unchanged

### Requirement: What counts as a write
The following SHALL be writes and SHALL make the written list, map or buffer unique first: index assignment `xs[i] = v` and `m[k] = v`; `push`, `add`, `take` and `clear` on lists; `remove` and `clear` on maps and every mutating buffer operation; compound assignment on an element (`xs[i] += 1`); and an assignment through a path (`a.items[2].n = 5`, `b.m["k"] = v`, `r.inner.list.push(x)`), where every list, map and buffer on the path SHALL be made unique before the final store. Reads (indexing, `len`, `has`, `get`, iteration, passing to a read-only parameter) SHALL NOT copy. Assigning to a record field that itself holds a record SHALL replace that field as before and SHALL NOT copy any list or map.

#### Scenario: Each write kind
- **WHEN** a program copies a list and a map and, on the copy, performs an index assignment, a push, a take, a clear, a map set, a map remove and a map clear, one program per operation
- **THEN** in each case the original is unchanged

#### Scenario: Path write
- **WHEN** a program copies `a: Outer` where `Outer` holds `[]Inner` and `Inner` holds a list, and writes `b.items[0].tags.push("x")`
- **THEN** `a.items[0].tags` has no `"x"` and `b.items[0].tags` has it

#### Scenario: Reads do not copy
- **WHEN** a program copies a list, then reads its length, indexes it and loops over it, and reads `live_objects()` before and after
- **THEN** the count has not changed

### Requirement: ref parameters refer to the caller's value
A `ref` parameter or receiver SHALL refer to the caller's place, not to a copy. A write through it SHALL be visible to the caller, and SHALL copy the list, map or buffer only when that storage is shared with some other holder, in which case the caller's place receives the new copy and the other holder keeps the old one.

#### Scenario: Written through ref
- **WHEN** `func fill(ref xs: []int)` appends to `xs` and the caller reads its list afterwards
- **THEN** the caller sees the appended elements, and `live_objects()` did not grow when the caller's list was unique

#### Scenario: Shared storage behind a ref
- **WHEN** `ys` is a copy of `xs` and `fill(ref ys)` appends
- **THEN** `ys` has the new elements and `xs` does not

### Requirement: Iteration while mutating
A `for` loop over a list or map SHALL visit the elements the collection had when the loop started, even if the loop body writes to the collection through the same name. Such a write SHALL be allowed and SHALL copy the collection once, because the loop holds its own reference; it SHALL NOT crash, skip or repeat elements, and SHALL NOT be a compile error.

#### Scenario: Append inside the loop
- **WHEN** a loop over `xs = [1, 2, 3]` pushes each element again
- **THEN** the loop runs three times and `xs` ends with six elements

#### Scenario: Remove inside a map loop
- **WHEN** a loop over a map with three keys removes each key it visits
- **THEN** the loop visits all three keys and the map ends empty

### Requirement: No memory leaks and no double releases
Every copy made by a write SHALL be released when its last holder goes away, and the storage it replaced SHALL lose exactly the writer's reference. A program that copies and writes records with lists, maps and buffers repeatedly SHALL leave `live_objects()` unchanged between iterations, with and without optimization.

#### Scenario: Copy and write in a loop
- **WHEN** a program copies a record holding a list of texts and a map of records, writes to both copies, and does this 5,000 times
- **THEN** `live_objects()` does not grow between the first and the last iteration

#### Scenario: Error paths
- **WHEN** a write to a shared list stops with a runtime error after the copy was made (for example an index out of range in a path write)
- **THEN** the program reports the same error as before this change and exits with the same status

### Requirement: Same results with and without optimization
With optimization on, the compiler SHALL produce the same observable behavior as with `--no-opt` for programs that copy and write lists, maps and buffers, including when the value is held in a register-allocated variable, passed to an inlined function, or captured by a closure.

#### Scenario: Differential check
- **WHEN** the copy-on-write test programs are compiled with and without optimization
- **THEN** their output and exit status are identical

### Requirement: The cost is documented
The README and the language documentation SHALL state that copying a record or list is cheap, that writing to a list, map or buffer that another holder still has copies it, and that `ref` avoids the copy when the caller wants its own value changed.

#### Scenario: Docs mention the cost
- **WHEN** `docs/native.html`, `docs/syntax.html` and the README are searched for the cost
- **THEN** each describes "writing to a shared list copies it" and no longer says lists or maps are shared references
