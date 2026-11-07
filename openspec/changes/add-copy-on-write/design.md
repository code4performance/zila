## Context

What happens today, from `src/codegen_x64.rs`:

- Heap objects (text, records, lists, maps, closures, ...) have a 16-byte header `[refcount][drop fn]`; `rt_retain` increments and `rt_release` frees at zero. A list is `[len][cap][data*][elements_are_heap]`; a map is a separate object driven by `rt_map_*`.
- `value()` (used when a value is stored: assignment, argument, return, field, element) deep-copies a plain record read from a place with `B_copy_<Record>` (`gen_record_helpers`). The same happens for loop variables (`bind_loop_var`), closure captures and list-iteration elements. Records marked `#[shared]` (`shared_recs`) are never copied.
- `B_copy_<Record>` copies the bytes with `rt_memcpy`, recursively copies fields that are plain records, and only calls `rt_retain` for every other heap field. So a list, map or text inside a copied record is shared: the record is a value, but its list is an alias. `let ys = xs` and passing a list or map retain and share as well (the `maps` spec and `docs/native.html` say so explicitly).
- Writes go straight to the shared object: `assign` (`Index` target calls `rt_list_set` or `rt_map_set`, `Field` target stores into the record), list methods `push`/`add`/`clear`/`take` (`rt_list_push`, `rt_list_clear`, `rt_list_take`), and `map_method` (`rt_map_remove`, `rt_map_clear`).
- `ref` parameters and receivers pass the original record pointer; `async` functions forbid them.
- `std/sorted.br` and `std/set.br` use the sharing: `walk_keys(n, out: []K)` fills a caller's list, `Set<T>` is a record around a map and behaves as a reference.

So the gap is exactly "heap fields of a copied record, and lists/maps themselves, alias". Reference counts are already correct (the leak tests in `tests/maps.rs` show that), which is what makes copy-on-write cheap to add.

## Goals / Non-Goals

**Goals**
- Value semantics for every list, map, mutable text buffer and record containing them, with no copy until a write.
- Unique values are written in place at today's speed plus one count check.
- No leaks, no double release, same behavior with and without optimization.
- All sharing of mutable state is explicit (`ref`, `new T`, `shared T`).

**Non-Goals**
- Copy-on-write for records themselves: plain records are still copied eagerly on assignment (nested plain records recursively), as today. Making record copies lazy is possible later, but records are small and fixed-size, so the cost is bounded by the record size, not by data.
- Making text mutable (see `add-text-bytes-and-char` for the buffer type; this change only fixes its sharing rule). Text values stay immutable and shared.
- Thread safety of reference counts (`add-threads-and-shared`).
- Detecting "iterating while mutating" as an error: it is defined behavior (snapshot).
- Changing the interpreter (`remove-interpreter` deletes it).

## Decisions

1. **Rule.** A *write* to a list, map or buffer object `o` reached through a place `p` (a local slot, a global, a record field, a list element, a map value, a `ref` parameter's referent) runs `unique(p)`: if `o.refcount == 1` nothing happens; otherwise `o' = clone(o)` is made with refcount 1, `p` is set to `o'` and `o` loses one reference (`rt_release`, which cannot free it because another holder exists). The write then proceeds on `o'`. The check is a load and compare of `[o - 16]`.

2. **The write needs a place, not a pointer.** Today `assign` and the list method branches evaluate the container into an owned temporary (`operand`) and call `rt_list_set(rcx, ...)`. The temporary holds +1, which would make every list look shared. The change lowers writes in two steps: (a) compute the *address of the owning slot* (`lea` of the local, global symbol, `record + field offset`, or element slot of the enclosing list/map after that one was made unique) with a new `place_addr(expr)` helper; (b) call `rt_list_unique(slot_addr, clone_fn)`, which returns the (possibly new) list in `rax`, then perform the write. For a path `a.items[2].tags.push(x)` the helper walks from the root: root place, make unique if it is a list/map/buffer (a record root is a place already and needs nothing), then each field offset or element slot, making each list/map on the path unique before descending. Plain records on the path are unique by construction (they are copied eagerly, or shared records, which are intentionally shared and not cloned). Reads keep using `operand` and borrow without touching counts.

3. **Clone helpers per element type.** A clone makes a new object and copies the element words, then fixes ownership per element: elements that are plain records are copied with `B_copy_<Record>` (they must not alias, since a record write reaches them through the list), shared records, text, closures, enums and the like are retained, and a nested list or map is retained (it will itself be copied by its own first write, so nesting costs one level at a time, not a deep copy). `rt_list_new` already stores `elements_are_heap`; the codegen passes a per-element-type clone function (generated next to `gen_record_helpers`, `B_lclone_<T>`) or `0` when the elements are plain words and a `memcpy` suffices. Maps do the same for values, keys being text (immutable, retained) or integers; the clone must keep insertion order, tombstones may be compacted.

4. **Record copy.** `B_copy_<Record>` is unchanged in structure and its comment is updated: it is the shallow copy, retaining lists, maps, buffers and text, copying nested plain records. After decision 1 the retain is exactly the "copy" of a list. Closure capture, loop variables and list-element reads already go through `value()` / `B_copy_`, so they gain value semantics without separate work.

5. **Plain lists and maps are values, too.** `let ys = xs`, argument passing and returning retain (as now) and no longer alias observably, because every write is guarded. The statement in the maps, sets and docs that they are shared references is replaced. This is the breaking part.

6. **`ref`.** A `ref` parameter (and receiver) is passed as the address of the caller's place (a record pointer today; for list/map/buffer parameters the compiler passes the address of the slot). A write inside the callee runs `unique` on that slot, so the caller's own variable is updated, and copies only if the storage is shared. For records passed by `ref`, `place_addr` through the pointer reaches the caller's fields, so a path write inside the callee makes the caller's lists unique in the caller's record. `async` functions keep forbidding `ref`.

7. **Iteration while mutating.** The `for` loop already evaluates the iterable into a hidden local with an owned +1 (`hidden_local(lt)` in `for_stmt` for maps; lists use a slot too). Therefore a write to the same variable inside the body sees refcount 2, copies once and updates the variable, and the loop continues over the original (snapshot). No code is needed beyond making sure the hidden reference is held (a task verifies it for lists, maps and `for k in m`). Cost to document: the first write in such a loop is O(n). Programs that want in-place mutation index by position (`while i < xs.len`) instead.

8. **`live_objects()`.** `rt_live` counts objects through `rt_alloc`/`rt_free_obj`; list data buffers are not objects of their own. A clone allocates exactly one counted object, so "one write to a shared list grows the count by one and later writes by none" is observable; copies of records with lists add the record only. Tests use this for the "no copy when unique" and the leak-freedom checks.

9. **Optimizer implications.** (a) *Register allocation* (`src/regalloc.rs`) only allocates integers, booleans and pointers, so no list slot is ever in a register; but `place_addr` takes the address of a slot and `regalloc` marks as `bad` names whose address is taken, so the planner must treat "write to a list/map/buffer by name" like an address-taken name for list-typed variables (which already cannot be candidates by type). A task adds a regression test rather than a change if the type filter holds. (b) *Inlining* (`inline_call`, `inline_bodies`) is restricted to plain numbers, booleans and raw pointers, so an inlined body never writes a list; the alias shortcut for arguments is unaffected. The task verifies this with an assembly test and a differential test, and records the rule that any later widening of inlining to heap parameters must keep the argument's +1 so `unique` sees the right count. (c) The "borrowed read" optimization in `operand`/`value` (reads of a local that is only inspected do not retain) stays valid because borrows never write. A write while a borrow is live (for instance `xs[i] = f(xs)` where `f` borrows `xs`) is the risky case: the argument to `f` is handed over with +1, so the write sees the count raised and copies, which is the correct value semantics.

10. **Read-then-write in one statement.** `xs[i] = xs[j] + 1` evaluates the right-hand side first (it is a value), then calls `unique` on `xs` and stores. Order of evaluation of the place's index expression versus the right-hand side stays as `assign` has it today (container, index, then value), but `unique` runs after the value is computed, immediately before the store, so a right-hand side that captures or copies the list cannot leave a stale pointer.

11. **Standard library and other code.** An audit (task 5) lists every function in `std/`, `os/` and `examples/` that takes a list, map or set parameter and mutates it for the caller, or that stores one in a record and expects aliasing. Each becomes `ref`, returns the changed value, or the type becomes shared. Known cases: the `out: []K/V/Entry<K,V>` parameters of `walk_keys`, `walk_values`, `walk_entries`, `walk_range` in `std/sorted.br`; `Set` methods (receivers refer to the caller's value, so they keep working); the internals of `std/list.br`, `std/text.br`, `std/format.br`, `std/bufio.br` that build into lists passed down helpers.

12. **Cost and diagnostics.** The documented cost is "writing to a shared list copies it". No warning is emitted; a later change may add a debug counter.

13. **Errors.** A runtime error in the write (index out of range) occurs before or after `unique`? The bounds check runs first, on the old object, so an out-of-range write neither copies nor changes the program's error output; the scenario "error paths" holds.

## Depends on

- `add-direct-pe-output`: independent of this change at the semantic level; it only affects how the test executables are built. Tests here use the compiler's normal build path, whichever is current.
- `remove-interpreter`: removes `src/interp.rs` and the "interpreter agrees" requirements. If it lands first, drop nothing here; if this lands first, the interpreter keeps its shared-list behavior until it is removed and no differential test against it is added.
- `add-weak-refs` (`new T`) and `add-threads-and-shared` (`shared T`): define the only exceptions to value semantics. This change keeps `#[shared]` as the interim spelling and states the rule in terms of the future keywords; their changes replace `#[shared]`. They must not give `new T` values copy-on-write.
- `add-text-bytes-and-char`: introduces the mutable text buffer whose writes are covered here by the same rule (`unique` plus a per-type clone). Until it lands, the buffer clauses of the spec have no program to exercise them and the tasks for buffers are deferred to that change.
- `add-error-kind`, `remove-panic-add-abort`, `remove-async-coloring`, `add-int-long-aliases`, `add-visibility-pub`, `add-package-manager`, `add-comptime-reflection`, `add-json`, `add-web`, `restrict-ref-escape`, `add-no-alloc-and-allocator`, `add-derived-eq-ord-hash`: no code dependency. `restrict-ref-escape` interacts with decision 6 (a `ref` to a place that unique may replace must not outlive the place) and should build on the place-address lowering. `add-no-alloc-and-allocator` must treat a copy-on-write clone as an allocation. `add-derived-eq-ord-hash` compares and hashes values, which is meaningful because of value semantics. `add-json` and `add-web` are library code written under these rules.

## Risks / Trade-offs

- **Silent behavior change** for code that relied on aliasing: a callee that appended to a list now changes nothing for the caller. Mitigation: the audit task, `ref` migration notes in the docs, and tests that fail loudly in `std/` and `examples/`.
- **Accidental O(n) copies** in loops that write to a list that is also held elsewhere (for example a list stored in a record and also in a local). Mitigation: documentation of the cost, a test that a unique append loop does not allocate, and the place-based lowering so locals are not retained by temporaries.
- **Count off-by-one** (a temporary that holds +1 across the write makes everything look shared and defeats the in-place path, or a missing retain frees a shared list). Mitigation: the `live_objects()` tests for both directions, and a debug assertion mode (`rt_live`) in the leak tests.
- **Nested clones** are one level at a time, so a deep structure written at depth copies only the path from the root to the write, not the whole tree; that is the intended behavior and is covered by the path-write tests.
- **Code size**: one clone helper per element type used by a list or map. Generated lazily like `B_copy_*`.

## Migration

Pre-1.0 policy (GOVERNANCE.md) allows the break. Migration note for the changelog and docs: a function that fills a caller's list or map must declare the parameter `ref` and the caller must write `ref` at the call site, or return the new list. Code that wants two names for one mutable collection should use a record created with `new` (`add-weak-refs`) or `shared` (`add-threads-and-shared`).
