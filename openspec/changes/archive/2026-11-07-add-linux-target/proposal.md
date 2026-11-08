## Why

Zila Lang compiles only to Windows executables (and a bare-metal kernel). The roadmap's "more platforms" item starts with Linux x86-64. Linux is also the cheapest first step: the compiler already has its own x86-64 assembler (it builds the bare-metal kernel without NASM), a Linux executable needs no linker and no C library (a static ELF that talks to the kernel with system calls), and Windows can run the result through WSL for testing.

## What Changes

- A new option `--target linux`: `zila-parse prog.br --target linux -o prog` writes a static x86-64 ELF executable, using only the compiler (no Visual Studio, no linker, no libc). `--asm --target linux` shows the generated assembly. The default target stays Windows.
- A Linux runtime written in assembly next to the Windows and bare-metal ones: output through `write`, exit through `exit_group`, and memory from the kernel (a zeroed heap region in the executable's own address space for small objects, and `mmap`/`munmap` for large ones so that big texts and lists are given back).
- Programs that use `print`, text, lists, maps, records, enums, generics, interfaces, closures, `defer`, errors with `try` and `match`, floats, `unsafe` and `asm`, and the pure-Zila Lang libraries (`text`, `list`, `math`, `float`, `format`, `set`, `sorted`, `bufio`) run on Linux and print what they print on Windows. A program that returns an error from `main` or panics exits the same way (`error: ...` with status 1, `panic: ...` with status 101).
- What needs the Windows API is not available on Linux yet and is rejected with a message that says so: `use fs`, `net`, `task`, `ui` (so `async`, `spawn` and channels), and `extern` functions (with `#[link]`).
- A new section of the documentation explains the target and its limits; the roadmap row is updated.

## Capabilities

### New Capabilities

- `linux-target`: building Linux executables, the Linux runtime, and what is and is not available there.

### Modified Capabilities

None.

## Impact

- `src/asm_x86.rs` (the `syscall` instruction), `src/codegen_x64.rs` (a `Linux` target with its own start-up code and runtime, library gating, an ELF writer), `src/cli.rs` and `src/main.rs` (`--target`).
- New `tests/linux.rs` (runs the executables through WSL and skips without it), `docs/native.html`, `docs/getting-started.html` (the option list that `tests/docs.rs` checks), README.
