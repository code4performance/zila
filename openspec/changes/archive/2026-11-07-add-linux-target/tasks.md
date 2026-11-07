## 1. Assembler, target and runtime

- [x] 1.1 `syscall` in the assembler (with a test)
- [x] 1.2 `Target::Linux`: library and `extern` gating with the shared message, review of the bare-metal-only checks (maps and floats allowed)
- [x] 1.3 The Linux runtime: `rt_write`, `rt_exit`, the allocator (size classes over the zeroed heap, `mmap`/`munmap` for large blocks), `rt_write_out`, and the start-up code
- [x] 1.4 The ELF writer (headers, one loadable segment, entry from the symbol table) with unit tests of the header

## 2. Command line

- [x] 2.1 `--target <windows|linux>` in `src/cli.rs` and `src/main.rs` (building and `--asm`, errors for unknown targets), and the usage text

## 3. Tests and documentation

- [x] 3.1 `tests/linux.rs`: hello, exit codes and panics, errors, memory (loops, big values, `live_objects`), examples compared with Windows output, rejected libraries and `extern`, bad target; all run under WSL and skip without it
- [x] 3.2 Prove the default is unchanged: the whole existing suite passes and the boot image is the same
- [x] 3.3 `docs/native.html` (the Linux target and its limits), `docs/getting-started.html` (`--target`) and README rows
- [x] 3.4 Full test suite (async included, since the runtime shares code) and `openspec validate add-linux-target`
