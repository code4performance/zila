## Why

The README promises that allocation only happens in known operations (text interpolation, list and map literals, closures that capture, `spawn`, `new`) and that the allocator can be replaced. Neither is enforceable today. A function that must not allocate (an interrupt handler, the allocator itself, a hot loop) has no way to say so, and the compiler cannot check it. The runtime allocator is fixed per target (`HeapAlloc` on Windows, a free-list and `mmap` heap on Linux, a 64 MiB bump heap after the image on bare metal), so a kernel cannot put its own physical-memory allocator behind lists, text and records, and the kernel's heap is baked into the compiler's runtime text instead of living in `os/kernel.br`.

## What Changes

- **BREAKING (pre-1.0)**: new attribute `#[no_alloc]` on functions. The compiler rejects, inside such a function, every allocating operation: text interpolation, list and map literals, `new`, closures that capture, `spawn`, calls to functions that allocate (found by a transitive analysis over the call graph), copy-on-write writes that may copy, and generic instantiations that allocate. Calls through function values and interface methods are rejected. The diagnostic names the offending expression and the call chain.
- The standard library is analysed like user code: there is no hand-kept list of allocating functions. `#[no_alloc]` on a std function is a promise the compiler checks.
- New attributes `#[allocator(alloc)]`, `#[allocator(free)]` and optional `#[allocator(realloc)]` on three functions with fixed signatures. A program that declares them replaces the runtime's heap on every target. At most one set per program. The allocator functions are implicitly `#[no_alloc]`.
- `live_objects()` keeps counting (the runtime wraps the custom allocator); `heap_bytes()` is rejected when a custom allocator is present. Out of memory (the allocator returns null) aborts through the runtime's abort path.
- `os/kernel.br` moves its heap behind `#[allocator]`; std functions that cannot allocate are marked `#[no_alloc]` so the compiler keeps them that way; README, docs and the VS Code grammar cover both attributes.

## Capabilities

### New Capabilities

- `no-alloc`: the `#[no_alloc]` attribute, what counts as an allocating operation, the transitive analysis, its limits and the diagnostics.
- `custom-allocator`: `#[allocator]`, its signatures, per-target rules, uniqueness, `live_objects()`, `heap_bytes()` and out-of-memory behavior.

### Modified Capabilities

- `linux-target`: the Linux runtime heap (free lists and `mmap`) is only the default allocator.

## Impact

- `src/codegen_x64.rs`: `rt_alloc` and `rt_free_obj` in `RUNTIME_WIN`, the Linux runtime and `RUNTIME_BARE`, `RUNTIME_DATA_*`, `heap_bytes`, `c_intrinsic` (`c.alloc`, `c.free`), `alloc()`, attribute handling next to `naked`/`shared`; a new analysis module `src/noalloc.rs` called before code generation.
- `std/*.br` (mark functions that cannot allocate), `os/kernel.br`, `examples/no_alloc.br` and `examples/allocator.br` (new), `tests/noalloc.rs` and `tests/allocator.rs` (new), `tests/os.rs`, `README.md`, `docs/syntax.html`, `docs/native.html`, `docs/os.html`, `editors/vscode/syntaxes/zila.tmLanguage.json`.
- Depends on other planned changes (see design.md): `add-direct-pe-output`, `remove-panic-add-abort`, `add-copy-on-write`, `remove-interpreter`.
