# custom-allocator Specification

## Purpose

Lets a program replace the runtime heap on every target with its own allocator functions while the runtime keeps owning object headers, reference counts and the live count.

## Requirements

### Requirement: Allocator functions
A program SHALL be able to replace the runtime heap by declaring top-level functions with these attributes and signatures: `#[allocator(alloc)] func f(size: u64, align: u64) -> *u8` (returns null on failure), `#[allocator(free)] func f(ptr: *u8, size: u64, align: u64)`, and optionally `#[allocator(realloc)] func f(ptr: *u8, old: u64, size: u64, align: u64) -> *u8`. A wrong signature SHALL be an error that shows the expected one. The functions SHALL behave as if they were `#[no_alloc]`, so allocating inside them is a compile error.

#### Scenario: Valid allocator
- **WHEN** a program declares `alloc` and `free` functions with the signatures above and builds a list of numbers
- **THEN** the list's memory is obtained from the `alloc` function and returned to the `free` function

#### Scenario: Wrong signature
- **WHEN** an `#[allocator(alloc)]` function takes one parameter
- **THEN** compilation fails showing the expected signature

#### Scenario: Allocator allocates
- **WHEN** an allocator function contains a text interpolation
- **THEN** compilation fails with the no-alloc diagnostic

### Requirement: One allocator per program
A program SHALL contain at most one function for each allocator role, and SHALL declare either none or both of `alloc` and `free`; `realloc` SHALL require both. The attributes SHALL be accepted only on top-level functions of the program being built, not of the standard library. A duplicate SHALL be an error naming both declarations.

#### Scenario: Duplicate
- **WHEN** two functions are marked `#[allocator(alloc)]`
- **THEN** compilation fails naming both

#### Scenario: Missing free
- **WHEN** a program declares `alloc` and no `free`
- **THEN** compilation fails saying `free` is required

#### Scenario: In the standard library
- **WHEN** a file under `std/` carries `#[allocator(alloc)]`
- **THEN** compilation fails saying the attribute is for the program

### Requirement: Memory contents and layout
The runtime SHALL keep owning object headers, reference counts, zeroing and the live count: it SHALL request `payload + 24` bytes with alignment 16, zero the payload and write the header itself. The allocator MAY return uninitialized memory. `free` SHALL receive the same size and alignment that `alloc` was asked for. When no `realloc` is declared, growth SHALL be done by allocating, copying and freeing.

#### Scenario: Zeroed objects
- **WHEN** a custom allocator returns memory full of `0xFF` and the program creates a record with numeric fields
- **THEN** the fields read as zero

#### Scenario: Free receives the same size
- **WHEN** an object is created and released
- **THEN** `free` is called once with the pointer, size and alignment given to `alloc`

### Requirement: Targets
A custom allocator SHALL be usable on every target (Windows, Linux and bare metal) and SHALL replace the built-in heap there. When one is declared, the Windows program SHALL NOT import `GetProcessHeap`, `HeapAlloc` or `HeapFree`, and the Linux program SHALL NOT contain its free-list heap or its `mmap` and `munmap` code. A bare-metal program without an allocator SHALL keep the built-in heap, and `os/kernel.zila` SHALL provide its own through these attributes.

#### Scenario: Windows without the process heap
- **WHEN** a Windows program with a custom allocator is built
- **THEN** the imports of the executable contain no `HeapAlloc` and the program runs correctly

#### Scenario: Kernel provides the allocator
- **WHEN** the kernel image is built and booted
- **THEN** its lists, text and records are allocated by the functions in `os/kernel.zila` and the boot test passes

### Requirement: live_objects and heap_bytes
`live_objects()` SHALL count objects with a custom allocator as it does with the built-in one. `heap_bytes()` SHALL be a compile error in a program that declares an allocator, with a message that it needs the built-in heap.

#### Scenario: Leak check still works
- **WHEN** a program with a custom allocator creates and drops a thousand lists and compares `live_objects()` before and after
- **THEN** the difference is zero

#### Scenario: heap_bytes with custom allocator
- **WHEN** a bare-metal program declares an allocator and calls `heap_bytes()`
- **THEN** compilation fails saying it needs the built-in heap

### Requirement: Out of memory
When `alloc` or `realloc` returns null, the runtime SHALL stop the program through its abort path with the message `out of memory`, as the built-in heaps do. The allocator MAY stop the program itself with its own message first.

#### Scenario: Allocator returns null
- **WHEN** a custom `alloc` always returns null and the program creates a list
- **THEN** the program ends with `out of memory` and a failure exit status
