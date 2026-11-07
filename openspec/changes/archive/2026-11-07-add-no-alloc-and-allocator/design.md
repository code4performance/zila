## Context

Allocation goes through one runtime procedure, `rt_alloc` in `src/codegen_x64.rs` (rcx = payload size, rdx = drop procedure, rax = zeroed payload), and one release procedure, `rt_free_obj` (rcx = payload). Objects have a header of refcount and drop procedure (16 bytes on Windows; 24 on Linux and bare metal, which also store the block size). Per target:

- Windows (`RUNTIME_WIN`): `HeapAlloc` on the process heap (`rt_heap`), `HeapFree` in `rt_free_obj`.
- Linux: size-class free lists (`rt_freelists`), a bump pointer (`rt_brk`, limit `rt_heap_end`) over zeroed memory after the image, and one `mmap` per block of 4096 bytes or more (`munmap` when freed).
- Bare metal (`RUNTIME_BARE`): the same size classes and bump pointer starting at 2 MiB, limited to 64 MiB. There is no OS heap, so the heap is part of the runtime text. `heap_bytes()` (kernel only) reads `rt_brk`.

`rt_live` is incremented in `rt_alloc` and decremented in `rt_free_obj`; `live_objects()` reads it. Out of memory ends in `rt_alloc_fail`, which calls `rt_panic` with `rt_msg_oom`. Allocation sites in the generator are `alloc()` (records, closures, lists, text) and `c.alloc` in `c_intrinsic`. Attributes are parsed generically into `Attr { name, args }` by `Parser::attrs` and looked up by name in the generator (`naked`, `shared`, `link`).

**Depends on:**
- `add-direct-pe-output`: the Windows import table is built from the symbols the program uses; a custom allocator must not cause `GetProcessHeap`, `HeapAlloc` and `HeapFree` to be imported.
- `remove-panic-add-abort`: out of memory becomes `abort("out of memory")`; until it lands the existing `rt_panic` path is used.
- `add-copy-on-write`: defines which writes may copy (decision 4).
- `remove-interpreter`: the analysis and the allocator exist only in the native compiler, so there is nothing to keep in sync.
- `remove-async-coloring` and `add-threads-and-shared`: they change which forms create tasks and shared values; the analysis treats whatever forms exist at implementation time as `spawn`-like allocation.
- `add-text-bytes-and-char`: constant text and bytes literals do not allocate.
- `add-weak-refs`: creating or upgrading a weak reference does not allocate.

## Goals / Non-Goals

**Goals**
- A compiler-checked guarantee that a function allocates nothing, with a call-chain diagnostic.
- A program, most importantly the kernel, can supply the heap primitives.
- No hand-maintained list of allocating functions.

**Non-Goals**
- Stack-use limits, bounded run time or "no abort" checking.
- Several allocators per program, per-type or per-scope allocators, arenas in the language (they can be built in a program on top of one `#[allocator]`).
- Proving anything about `asm` or `unsafe` code beyond the rules below.

## Decisions

**1. The allocating operations are syntactic and closed.** In a function body these allocate: a text with at least one interpolation, a list, map or set literal with elements and any call that creates a collection, `new`, a closure that captures at least one variable, `spawn` and `channel`, `defer` (it creates the defer list, see `init_defers`), `c.alloc`, a write that may copy (decision 4) and a call to an allocating function (decision 2). A text literal without interpolation is a constant in the image and does not allocate. A closure that captures nothing is a plain function pointer and does not allocate. Reference count changes do not allocate. Releasing a value may call the allocator's `free`; freeing is not allocating.

**2. Transitive analysis, once per compile.** A new module `src/noalloc.rs` walks every function body (std included, generics after instantiation) and records its direct allocating operations and its callees. `allocates(f)` is the least fixed point over the call graph, so recursion is handled. Each function with `#[no_alloc]` is checked: if `allocates(f)` holds, the error carries the shortest chain found by breadth-first search from `f` to a direct operation. The attribute is not required on callees; a callee that does not allocate is "provably non-allocating". Alternative: marking every allocating std function by hand. Rejected: the list goes stale, and std is Brasa source the compiler already reads. `extern` functions have no body and are trusted not to allocate (OS calls do not touch Brasa's heap); the few runtime hooks that do must be declared `#[allocates]`, which the analysis treats as a direct allocating operation.

**3. Limits.**
- A call through a function value (parameter, field, list element) inside `#[no_alloc]` is rejected: the callee is unknown. The program calls a named function instead.
- A call to an interface method is rejected for the same reason, unless the receiver's concrete type is known at the call site (static dispatch), in which case the method is analysed.
- A generic function is analysed per instantiation; the diagnostic names the instantiation (`push<i32>`), so one instantiation can be accepted and another rejected.
- `asm` blocks are trusted. A function whose `asm` calls `rt_alloc` must not be `#[no_alloc]`.
- Calling an async function creates a future, which allocates; it is rejected (`remove-async-coloring` may change this).
- `abort` and runtime failures such as index out of bounds are not allocation.

**4. Copy-on-write.** After `add-copy-on-write`, a write to a shared value may copy first. Inside `#[no_alloc]` a write to a list, text or record is accepted only when the compiler can see the value is uniquely owned (using the ownership facts that change exposes); otherwise it is rejected with "this write may copy". Until that change lands, writes are in place and nothing is rejected on this ground.

**5. Allocator signatures.**
```
#[allocator(alloc)]   func my_alloc(size: u64, align: u64) -> *u8
#[allocator(free)]    func my_free(ptr: *u8, size: u64, align: u64)
#[allocator(realloc)] func my_realloc(ptr: *u8, old: u64, size: u64, align: u64) -> *u8
```
`alloc` returns null on failure. The runtime keeps owning object headers, reference counts, zeroing and `rt_live`: `rt_alloc` becomes a wrapper that asks `alloc` for `size + 24` bytes (the header is a compiler constant on every target in the custom case), zeroes the payload, writes the header and increments `rt_live`. A custom allocator therefore returns uninitialized memory aligned to `align` (16 today) and never sees objects. `free` receives the exact size and alignment of the block. `realloc` is optional; when absent the runtime does alloc, copy, free (used by list growth). Alternative: letting the program own the header. Rejected: it exposes the object layout. The functions are ordinary Brasa functions with the target's usual calling convention, implicitly `#[no_alloc]`; a wrong signature is a compile error naming the expected one.

**6. Where it may appear.** The attributes are accepted on top-level functions of the program being built (not of std), so `os/kernel.br` can supply them. A program has either no allocator or both `alloc` and `free`; `realloc` needs both. A second declaration of the same role is an error naming both places. The allocator replaces the heap on every target, which makes it testable on the normal test targets: the Windows runtime then leaves out `GetProcessHeap`, `HeapAlloc` and `HeapFree`, and the Linux runtime leaves out its free lists and `mmap`/`munmap`, keeping `rt_alloc` and `rt_free_obj` as the single entry points for everything else.

**7. Bare metal.** The kernel's heap moves from `RUNTIME_BARE` into `os/kernel.br`, written with `unsafe` pointer arithmetic over the region starting at 2 MiB and the same size classes. `RUNTIME_BARE` keeps the built-in heap only for kernels that declare no allocator. `heap_bytes()` depends on the built-in heap, so with a custom allocator it is a compile error; the kernel keeps its own counter and prints that. `c.alloc` and `c.free` go through the runtime wrapper and so through the custom allocator.

**8. Failure.** A null result from `alloc` or `realloc` is out of memory: the wrapper calls the runtime abort path with "out of memory" (today `rt_alloc_fail` to `rt_panic`). The allocator may abort sooner with its own message.

**9. `live_objects()`** is unchanged because the wrapper counts. Raw blocks the program takes from its allocator outside `rt_alloc` are not objects and are not counted.

**10. Diagnostics.** A normal compile error at the `#[no_alloc]` function:
```
error: 'parse_line' is #[no_alloc] but allocates
  parse_line -> trim_all -> text interpolation at util.br:12
```
A direct operation gives a chain of one element and names it: "list literal", "closure that captures `n`", "call through function value `cb`", "interface call `Shape.area`".

## Risks / Trade-offs

- One extra pass over the call graph per compile; the per-function result is cached.
- Rejecting function values and interface calls rejects some programs that would not allocate. That is deliberate; a named non-allocating wrapper is the way out.
- Trusting externs and `asm` leaves holes; they are documented, and they are the places `unsafe` already marks.
- Moving the kernel heap into Brasa may slow the kernel or hide bugs; `tests/os.rs` boots the image, and the kernel's reported `live objects` and heap counters are compared before and after.
- Tying the wrapper to a fixed header size means a layout change touches every allocator user, which is acceptable because the compiler and runtime ship together.
