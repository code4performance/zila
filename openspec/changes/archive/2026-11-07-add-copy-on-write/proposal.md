## Why

Brasa promises that records have value semantics: a record is copied when it is assigned or passed, and `ref` is how you share. The promise stops at the first heap value. Today `B_copy_<Record>` copies nested plain records, but a `[]T`, a `Map<K,V>` or a text held in a field is only retained, so two "copies" of a record silently share one list:

```
let a = Bag { items: [1, 2] }
var b = a
b.items.push(3)       // a.items.len is now 3 as well
```

The same holds for `let ys = xs` and for passing a list to a function that appends to it. Lists and maps are documented as "shared like a closure" and the `maps` spec says "Maps are shared references". Programmers must therefore know, for every field, whether it is a copy or an alias, which is the surprise the language set out to remove. The obvious fix, deep-copying on every assignment and call, makes passing a record with a big list cost O(n) for a read-only use.

The reference counts needed for a cheaper answer already exist: every list and map is a heap object with a `[refcount][drop]` header. Copy-on-write uses them: a copy only retains, and the first write to a list or map that someone else also holds copies it first. Value semantics then hold everywhere, a copy costs nothing until a write, and a unique list is written in place exactly as today. This is also the foundation for the planned ownership model: `new T` (add-weak-refs) and `shared T` (add-threads-and-shared) become the only ways to share mutable state, which a thread and package model needs.

## What Changes

- **BREAKING**: lists, maps and mutable text buffers have value semantics. Assigning them, passing them to a function, returning them, storing them in a record field, a list element or a map value, and capturing them in a closure gives the new owner a logically independent value. Code that relied on a callee appending to the caller's list, or on two names for one map, must pass the value as `ref` (or return the changed value). Migration: add `ref` to the parameter and to the argument at the call site, or return the new value.
- Copying is a shallow copy plus reference counting: copying a record retains each list, map and buffer it holds (plain nested records are still copied, as they are today). No element is copied at that point.
- Every write to a list, map or buffer first checks its reference count; when it is greater than 1 the value is copied (one level: elements are retained or, for plain records, copied) and the writer's place is updated to its own copy. The writes are: index assignment, `push`/`add`, `take`, `clear`, map set, map `remove` and `clear`, and a field or element assignment that reaches into a nested record or list (`a.items[2].n = 5`, `b.m["k"] = v`), where every container on the path is made unique first.
- `ref` parameters and receivers refer to the caller's place, so a write through them is a write to the caller's value and copies only if that value is itself shared.
- A loop over a list or map holds its own reference, so a write to that collection inside the loop copies it and the loop keeps visiting the values it started with.
- `live_objects()` counts a copy made by a write like any other object, so tests can observe both that no copy happens when the value is unique and that none leaks.
- Only `new T` and `shared T` share mutable state; the existing `#[shared]` attribute used by `std/sorted.br`, files, sockets and futures stays as the interim spelling until those changes land.
- The standard library (`std/`), `os/`, `examples/`, tests and docs are rewritten where they relied on aliasing (for example `walk_keys(n, out: []K)` in `std/sorted.br`, which becomes `ref out: []K`).
- The docs state the cost plainly: writing to a shared list copies it.

## Capabilities

### New Capabilities

- `copy-on-write`: value semantics for records that hold lists, maps and buffers, the operations that count as writes, `ref` behavior, iteration while mutating, `live_objects()` behavior, and the documented cost.

### Modified Capabilities

- `maps`: "Maps are shared references" is renamed and replaced by value semantics.
- `set-library`: a `Set<T>` is no longer a shared reference.
- `sorted-library`: states that sorted collections stay shared references (they are `#[shared]` records) rather than "like a map".
- `list-library`: the ownership requirement also covers independence of results and arguments.

## Impact

- `src/codegen_x64.rs`: a unique-or-copy step before each write (`assign` for `Index` and `Field` targets, the `Ty::List` branch of method calls, `map_method`), per-element-type list and map clone helpers generated next to `gen_record_helpers`, runtime procedures next to `rt_list_*` and `rt_map_*`, updated value-model comments, and a place-based lowering of nested writes.
- `src/regalloc.rs` and the inliner (`inline_call`): the unique check needs the owning place, so the rules about what may stay in a register or be aliased are re-checked (see design).
- `src/interp.rs` is removed by `remove-interpreter`; until then it keeps its own semantics and is not changed here.
- `std/` (`sorted.br`, `list.br`, `set.br`, `text.br`, `format.br`, `bufio.br`, others found by the audit task), `os/`, `examples/`, `tests/`, README and `docs/` (`native.html`, `syntax.html`, `stdlib.html`, `index.html`).
- Performance: programs that wrote to a list while another name held it now pay one O(n) copy; programs that did not are unchanged except for one reference-count test per write.
