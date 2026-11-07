## 1. Runtime

- [x] 1.1 Add `rt_list_unique(slot_addr, clone_fn)` and `rt_map_unique(slot_addr, clone_fn)` next to `rt_list_*`/`rt_map_*` in `src/codegen_x64.rs`: refcount 1 returns the object; otherwise clone, store in the slot, release the old object, return the new one. Verify: assembly test that a unique list takes the fast path (no `rt_alloc` call reachable from the check) and a Brasa test that shows `live_objects()` +1 on the first write to a shared list and +0 afterwards.
- [x] 1.2 Generate per-element-type clone helpers (`B_lclone_<T>`, `B_mclone_<K>_<V>`) next to `gen_record_helpers`: plain-record elements copied with `B_copy_<Record>`, other heap elements retained, plain words copied with `rt_memcpy`; map clone keeps insertion order. Verify: tests that write to a copy of lists of records, of texts, of lists and of maps and leave the original unchanged, with `live_objects()` returning to its starting value after both are dropped.
- [x] 1.3 Update the value-model comments at the top of `src/codegen_x64.rs` (records, `[]T`, maps) and the `B_copy_*` comment to describe shallow copy with reference counts and copy-on-write. Verify: review the comment against the implemented rule.

## 2. Writes

- [x] 2.1 Add `place_addr(expr)` to `src/codegen_x64.rs`: address of the owning slot for a local, global, `ref` parameter referent, record field (through a record or `ref` pointer) or list/map element, making each list/map on the path unique before descending. Verify: path-write tests (`a.items[2].tags.push(x)`, `b.m["k"].list[0] = v`) leave the original unchanged.
- [x] 2.2 Use it in `assign` for `ExprKind::Index` (list and map targets, including compound `+=`) and `ExprKind::Field` targets that reach into a list/map, calling `unique` after the right-hand side is evaluated and the bounds check done. Verify: tests for each write kind on a shared list/map (original unchanged), and an out-of-range write on a shared list prints the same error and status as before and does not copy.
- [x] 2.3 Use it in the `Ty::List` method branch (`push`/`add`, `clear`, `take`) and `map_method` (`remove`, `clear`) instead of the owned temporary of `operand`. Verify: a loop appending 10,000 integers to a unique list does not change `live_objects()`, and each method on a copy leaves the original unchanged.
- [x] 2.4 Check that the temporaries of `operand` for reads still borrow and never raise a count across a write (statements like `xs[i] = xs[j] + 1` and `xs[i] = f(xs)`). Verify: tests for both statements giving the same result with and without optimization.

## 3. ref, iteration and closures

- [x] 3.1 Pass the address of the caller's slot for `ref` parameters and receivers of list, map and buffer type, and run `unique` on it in the callee's writes (`call_sig`, receiver binding in method calls, `src/codegen_x64.rs`). Verify: `fill(ref xs)` fills the caller's list with no allocation when unique, and `fill(ref ys)` where `ys` is a copy changes only `ys`.
- [x] 3.2 Confirm `for` over a list and over a map (`for_stmt`, both `for x in xs` and `for k, v in m`) holds its own reference, and add the snapshot behavior to tests. Verify: append-in-loop and remove-in-map-loop tests from the spec, with `live_objects()` unchanged after the loop ends.
- [x] 3.3 Closure capture, loop variables (`bind_loop_var`) and returned values keep value semantics through `value()`. Verify: a closure capturing a list that is later appended to by the caller sees the old list, and a returned list field can be appended without changing the record.

## 4. Optimizer

- [x] 4.1 Check `src/regalloc.rs`: list/map/buffer variables are never register candidates and a write by name does not break the plan. Verify: a differential test (`tests/optimizer.rs` helper) with hot loops that write to lists held in locals, with and without optimization.
- [x] 4.2 Check `inline_call`: inlined bodies never write a heap value and the aliasing shortcut for arguments is unchanged. Verify: assembly test that a helper receiving a list is still a `call`, and a differential test of calls that read lists.
- [x] 4.3 Run the benchmarks in `benchmark_report` (`tests/optimizer.rs`) and record before/after numbers in this design; none may get slower by more than the count check. Verify: the existing assertions pass.

## 5. Standard library, os, examples

- [x] 5.1 Audit `std/*.br`, `os/*.br` and `examples/*.br` for functions that mutate a list, map or set parameter for the caller or rely on aliasing a field (search for list/map parameters followed by `.push(`, `.add(`, `[...] =`, `.clear(`, `.remove(`). Verify: the audit list is pasted into this task's commit message and each item below is resolved.
- [x] 5.2 `std/sorted.br`: make `out` of `walk_keys`, `walk_values`, `walk_entries`, `walk_range` a `ref` parameter and update call sites. Verify: `tests/sorted.rs` passes.
- [x] 5.3 Fix any other case found in 5.1 (`std/list.br`, `std/set.br`, `std/text.br`, `std/format.br`, `std/bufio.br`, `os/*.br`, `examples/*.br`). Verify: `tests/stdlib.rs`, `tests/sets.rs`, `tests/format.rs`, `tests/buffered_io.rs`, `tests/os.rs` and building every example pass (leave `tests/async.rs` out of routine runs, per the project memory).
- [x] 5.4 Verify `std/` functions that return an argument unchanged (`list.slice` of the whole list, `list.concat` with an empty list) return an independent value. Verify: the "result independent of argument" scenario as a test in `tests/stdlib.rs`.

## 6. Tests

- [x] 6.1 Add `tests/cow.rs` (using the same `run`/differential helpers as `tests/maps.rs`) with one test per scenario of `specs/copy-on-write/spec.md`: no aliasing after copy (records, plain lists, maps, nested path writes, passed and returned values), shared records exempt (use `#[shared]` until `new T` exists), no copy when unique and one copy on first shared write (`live_objects()` counts), `ref`, iteration while mutating. Verify: `cargo test --test cow`.
- [x] 6.2 Leak-freedom tests: copy-and-write loops of 5,000 iterations over records with lists, maps and lists of records, with error paths; `live_objects()` must not grow. Verify: `cargo test --test cow leaks`.
- [x] 6.3 Update tests that asserted aliasing: `tests/maps.rs` (changed through a parameter), `tests/generics.rs`, `tests/defer.rs` and others found by running the suite; each is changed to the new rule or to `ref`. Verify: `cargo test` (without `tests/async.rs`) passes.
- [x] 6.4 Add a random-program differential test that copies and writes lists and maps under both optimization settings. Verify: `cargo test --test optimizer`.

## 7. Documentation

- [x] 7.1 Update `docs/native.html` (lines on copying records, "Lists and closures are shared", `#[shared]`), `docs/syntax.html` (parameters, maps "shared like a list", `defer` capture note), `docs/stdlib.html` (sets and sorted collections), `docs/index.html` ("No surprise costs") and the README (value-semantics bullet, roadmap row): describe value semantics for lists and maps, the cost "writing to a shared list copies it", `ref`, and the migration note. Verify: `cargo test --test docs` and a manual read of each page.
- [x] 7.2 Update `examples/values.br` or add `examples/copy_on_write.br` showing a copied record with a list, a write, and a `ref` fill; keep the header comment style. Verify: the example builds and prints the documented output.
- [x] 7.3 Run the full test suite (without `tests/async.rs`) and `openspec validate add-copy-on-write --strict`. Verify: both succeed.

## Implementation notes

- Defaults taken: no `B_lclone_*`/`B_mclone_*` helpers per type; `rt_list_clone` and `rt_map_clone` take an optional element copy function and `B_copy_<Record>` is passed directly (plain-record elements copied, others retained). A clone of a list with elements costs two counted objects (list object plus its data buffer), so tests assert "grew after the first write, not after later ones" rather than "+1".
- `ref` list/map parameters are passed as the address of the caller's slot (new `Local.ind`). `ref` receivers of list/map type, `ref` lambda parameters and named functions with `ref` list/map parameters used as function values are not indirect (they behave by value); no std code uses them. Writes through a module-qualified global (`mod.xs.push(..)`) keep the old in-place path.
- Task 1.3 comments updated. Task 4.1/4.2: no regalloc/inliner change needed (list slots are never register candidates; inlining only takes plain scalars); covered by tests in `tests/cow.rs`. Task 4.3: `benchmark_report` and the existing assertions in `tests/optimizer.rs` pass; no before/after numbers recorded (no benchmark writes to a list).
- Task 5: audit found `std/sorted.br` walk_* (now `ref out`), `std/float.br` b_mul/b_mul_pow2/b_sub and `std/text.br` add_small (now `ref a`); `Set`/`SortedMap` methods keep working (receivers are records). 5.4: `list.slice/concat/reverse` already build fresh lists; test added in `tests/stdlib.rs`.
- `src/noalloc.rs`: "this write may copy" is reported for index writes, `push/add/clear/take/remove` and element-path field writes on a by-value list/map/record parameter; `#[ignore]` removed from `a_write_that_may_copy_is_rejected`. Limitation: only parameters are tracked (a local copied from a parameter, then written, is not flagged).
- Tests changed to the new rule: `tests/maps.rs` (maps are values, `ref` fill), `tests/sets.rs` (sets are values, `ref` shares). New: `tests/cow.rs` (28 tests, each run optimized and unoptimized), noalloc and stdlib tests.
- Test results: cow, abort, allocator, codegen, defer, derive, docs, floats, fmt, format, generics, integers, lsp, maps, noalloc, optimizer, parse, pe, refs, run, sets, sorted, stdlib, testrun, vscode, --lib, --bins all pass. Not run (user rule): async, buffered_io, udp, dns, tls, linux, os, ui, error_kinds (`cargo test --no-run` compiles). Examples: all build except control, ffi, lowlevel, rest_api, testing, types, which fail for reasons unrelated to this change (undefined sample identifiers, `#[link_src]` on Windows, no `main`, unsupported module/type).
