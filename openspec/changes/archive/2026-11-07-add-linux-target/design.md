## Context

`generate()` in `src/codegen_x64.rs` writes MASM-syntax assembly for a `Target` (`Windows`: imports from kernel32, linked by `ml64` and `link`; `Bare`: a freestanding kernel). For the kernel the compiler already runs its own assembler (`asm_x86::assemble(asm, base, 64)`), which returns the bytes and a symbol table, and writes a flat image. The code shared by all targets is `RUNTIME_COMMON` (reference counting, text, lists, integer and float conversions, `rt_panic`, which print through `rt_write` and stop through `rt_exit`) plus `RUNTIME_MAP` for maps. Each target adds `rt_alloc`, `rt_free_obj`, `rt_write_out`, `rt_exit` (and `rt_write` for Windows) and start-up code (`zila_start`). Heap blocks have the layout `[refcount][drop][payload]` on Windows and `[size][refcount][drop][payload]` on the bare target (the payload address is what code holds; refcount and drop sit at payload-16 and payload-8 in both).

## Goals / Non-Goals

**Goals**
- A Linux x86-64 executable from the compiler alone, with the same language semantics and output as Windows for the programs that can run there.

**Non-Goals**
- The libraries that sit on the Windows API (`fs`, `net`, `task` and what is built on it, `ui`), calling C, command-line arguments and environment, dynamic linking, other architectures, debugging information, running on macOS (a later step).

## Decisions

1. **A new target.** `Target::Linux` next to `Windows` and `Bare`. The code generator's checks (`Target::Bare` tests) are reviewed one by one: maps and floats are allowed on Linux (their runtime is plain assembly using `rt_alloc`), `heap_bytes()` stays kernel-only, the register allocator and optimizer stay on, and the library and `extern` gating (`ui`, `task`, `future`, `fs`, `net`, `sys`, `ws`) uses a shared message "'X' is not available on Linux yet".
2. **System calls.** The assembler gains `syscall` (`0F 05`). The runtime procedures keep the Zila Lang calling convention (arguments in `rcx`, `rdx`, ...) and translate to the Linux one inside: `rt_write` (`write`, number 1, looping until all bytes are written or an error), `rt_exit` (`exit_group`, 231), `mmap` (9) and `munmap` (11) for large blocks.
3. **Start-up.** `zila_start` is the first code in the image: `and rsp, -16`, `sub rsp, 48` (the kernel enters with the stack pointing at `argc`, 16-aligned), set up the heap pointers, run the module initializers, call `main`, the shared error/exit tail. `SetConsoleOutputCP` is Windows-only and is dropped.
4. **Memory.** A copy of the bare-metal allocator with two changes. Small blocks (under 4096 bytes) use the size-class free lists and a bump pointer over a zeroed region that the ELF header asks the kernel for beyond the file (`p_memsz` larger than `p_filesz`; the pages cost nothing until touched): 256 MiB, with `rt_brk` and a limit `rt_heap_end` set at start-up from the address of the end of the image. Larger blocks are individual `mmap` regions (zeroed by the kernel, their size in the block header) and `rt_free_obj` returns them with `munmap`. Running out of either gives `rt_panic("out of memory")`. The block layout is the bare-metal one, so the shared runtime works unchanged.
5. **The executable.** A single `PT_LOAD` segment, readable, writable and executable, `p_offset` 0 and `p_vaddr` `0x400000`, holding the 64-byte ELF header, the 56-byte program header and the assembled image (assembled with its origin at `0x400000 + 120`), with `p_memsz` = file size + heap size (rounded to pages) so the heap is the memory after the image. `e_entry` is the address of `zila_start` from the assembler's symbol table. The file is written with the executable bit where the platform has one. No section headers (not needed to run).
6. **Assembly differences.** The generated text uses MASM directives (`.code`, `.data`, `proc`/`endp`, `option casemap:none`), which the assembler already accepts for the kernel. Windows `extern` import lines and `.386`-style leftovers are not emitted for Linux.
7. **The CLI.** `--target <name>` (a value option, like `-o`), accepted values `windows` and `linux`; it applies to building (`-o`/`--build`) and to `--asm`. Other modes ignore it. The `--test` and `--run` modes are unaffected.
8. **Testing.** Tests build with `--target linux` and run the result under WSL (`wsl.exe`, copying the file into the Linux file system and marking it executable) and skip, with a message, when WSL is not usable. They cover hello, exit codes, panics, errors, memory (loops, big texts and lists, `live_objects`), the examples that do not use the Windows API compared with their Windows output, the rejected libraries and `extern`, the target option, and that the Windows build and the boot image are unchanged. The ELF writer and the new instruction also have unit tests that run without WSL.

## Risks / Trade-offs

- A single RWX segment is the simplest valid layout; hardened kernels that forbid it are not targeted.
- The 256 MiB small-object heap is a hard cap on live small objects; large objects are limited only by `mmap`.
- A division by zero ends the program with a signal (status 136) instead of a Windows exception; the message is not printed. Matching it would need a check in generated code and is left for later.
- Running the tests needs WSL; without it only the unit tests run, and the docs say the Linux executables were verified under WSL.
