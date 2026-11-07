## 1. Attributes and the analysis

- [x] 1.1 Record references before changing anything: the boot image size from `os/run.ps1`, the output of `os/kernel.br` (its `heap used` and `live objects` lines) and the generated assembly of one Windows and one Linux example. Verify: the files exist in the scratchpad for later comparison.
- [ ] 1.2 Validate `#[no_alloc]`, `#[allocates]` and `#[allocator(alloc|free|realloc)]` in the attribute handling of `src/codegen_x64.rs` (next to `naked`/`shared`): allowed targets, argument names, unknown role. Verify: `tests/noalloc.rs` cases for an attribute on a `type` and an unknown allocator role fail with the expected messages.
- [ ] 1.3 Add `src/noalloc.rs` (declare it in `src/lib.rs`): collect direct allocating operations and callees per function body, per generic instantiation, then compute `allocates` as a fixed point and report the shortest chain for every `#[no_alloc]` function. Verify: unit tests for chain text, recursion and a generic accepted for `i32` and rejected for another type argument.
- [ ] 1.4 Implement the syntactic rules of the spec: interpolation, collection literals, `new`, capturing closures (compare with the closure capture analysis near `rt_drop_closure` in `src/codegen_x64.rs`), `spawn`, `channel`, `defer`, `c.alloc`, async calls. Verify: one `tests/noalloc.rs` test per scenario of the no-alloc spec, including constant text and a non-capturing closure being accepted.
- [ ] 1.5 Reject calls through function values and interface methods with unknown receiver, accept static dispatch, trust `extern` unless `#[allocates]`, skip `asm` blocks. Verify: tests for the four scenarios and the error text naming the callee expression.
- [ ] 1.6 Reject writes that may copy using the ownership facts from `add-copy-on-write` (skip with a comment until that change is in; keep the test `#[ignore]`d with the reason). Verify: the copy scenarios pass once the dependency exists.
- [ ] 1.7 Call the analysis before code generation so errors carry `file:line` and show in `brasa` compile and in the language server (`src/lsp.rs` already reports compile errors). Verify: `tests/lsp.rs` gets one case that shows the diagnostic.

## 2. Custom allocator

- [ ] 2.1 Check the allocator declarations: signatures, uniqueness, alloc plus free, realloc needs both, only in the program and not in `std/`, implicitly `#[no_alloc]`. Verify: `tests/allocator.rs` cases for wrong signature, duplicate, missing free, std use, allocator that allocates.
- [ ] 2.2 Windows: when an allocator is present, emit the `rt_alloc` wrapper (request `size + 24`, align 16, zero, header, `inc rt_live`, null to `rt_alloc_fail`) and a `rt_free_obj` that calls `free`; drop `rt_heap`, `GetProcessHeap`, `HeapAlloc`, `HeapFree` from the runtime and the import list (coordinate with `add-direct-pe-output`'s symbol/DLL table). Verify: a test program with a counting bump allocator runs, prints the right output, and its imports lack `HeapAlloc`.
- [ ] 2.3 Linux: same wrapper in place of the free-list/`mmap` runtime and data (`rt_freelists`, `rt_brk`, `rt_heap_end`, the `mmap` in `rt_alloc_big`, `munmap` in `rt_free_obj`). Verify: the same test program built for Linux (in `tests/linux.rs`, run where Linux is available, else assert on generated assembly) and the assembly has no `syscall` for mmap.
- [ ] 2.4 Bare metal: the same wrapper in `RUNTIME_BARE`; keep the built-in heap when no allocator is declared. Verify: `tests/os.rs` still boots the unchanged kernel.
- [ ] 2.5 `realloc`: use it for list growth when declared, else alloc, copy, free. Verify: a test allocator counts calls and a growing list calls `realloc` (or alloc+free without it) as the spec says.
- [ ] 2.6 `live_objects()` unchanged through the wrapper; make `heap_bytes()` a compile error with a custom allocator (`src/codegen_x64.rs`, the `heap_bytes` branch near `live_objects`); `c.alloc`/`c.free` keep calling `rt_alloc`/`rt_release`. Verify: tests for the leak check (difference zero) and the `heap_bytes` error.
- [ ] 2.7 Out of memory: a null from `alloc`/`realloc` reaches `rt_alloc_fail` (the abort path from `remove-panic-add-abort` once present). Verify: a test with an allocator that returns null ends with `out of memory` and the failure exit status.

## 3. Rewrite the kernel, std and examples

- [ ] 3.1 Move the size-class heap from `RUNTIME_BARE` into `os/kernel.br` as `#[allocator]` functions with its own counter, and print that counter where `heap_bytes()` is printed now. Verify: `tests/os.rs` boots the kernel and the same output lines appear; compare the image size with the reference from 1.1.
- [ ] 3.2 Mark non-allocating functions in `std/*.br` (`text.br`, `list.br`, `math.br`, `float.br`, `sys.br`, ...) with `#[no_alloc]` where the compiler accepts it, and `#[allocates]` on the few extern hooks that allocate. Verify: `tests/stdlib.rs` and the whole test suite still compile every std module.
- [ ] 3.3 Add `examples/no_alloc.br` (a `#[no_alloc]` checksum/parse over fixed buffers) and `examples/allocator.br` (a bump allocator in an arena, counting calls); both run and print known output. Verify: tests in `tests/noalloc.rs` and `tests/allocator.rs` run them and compare output.

## 4. Documentation and editor

- [ ] 4.1 Describe both attributes, the limits and the signatures in `docs/syntax.html`, `docs/native.html` and `docs/os.html`; keep the README promise accurate (fixed list of allocating operations). Verify: `tests/docs.rs` passes (it compiles the examples in the docs).
- [ ] 4.2 Highlight `no_alloc`, `allocates` and `allocator` attributes in `editors/vscode/syntaxes/brasa.tmLanguage.json`. Verify: `tests/vscode.rs` passes.
- [ ] 4.3 Run the full suite (leaving `tests/async.rs` out of routine runs) and `openspec validate add-no-alloc-and-allocator --strict`. Verify: both pass.
