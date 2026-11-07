## MODIFIED Requirements

### Requirement: Memory
Objects SHALL be reference counted and freed as on Windows, `live_objects()` SHALL count them, and memory SHALL be given back: a loop that builds and drops large texts and lists SHALL NOT grow without bound. The built-in heap (size-class free lists over a region the executable asks the kernel for, large blocks from `mmap`) SHALL be the default allocator and SHALL be left out of the executable when the program declares a custom allocator with `#[allocator]`. Running out of memory SHALL stop the program with `panic: out of memory`.

#### Scenario: Out of memory
- **WHEN** a program keeps allocating small objects that stay alive until the heap is full
- **THEN** it prints `panic: out of memory` and exits with status 101

#### Scenario: No leaks and big values
- **WHEN** a program builds and drops 160,000-byte texts and lists of 2,000 elements hundreds of times, and a list of 300,000 integers is built and summed, and `live_objects()` is compared before and after
- **THEN** the count has not grown and the program finishes in reasonable memory

#### Scenario: Custom allocator replaces the heap
- **WHEN** a Linux program declares `#[allocator]` functions and builds lists and texts
- **THEN** their memory comes from those functions and the executable contains no `mmap` or `munmap` runtime code
