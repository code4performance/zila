## Context

The design of the debugger and of the extension is in `openspec/changes/archive/2026-11-15-add-editor-intelligence/design.md` (decisions 5 to 8) and what was built in its `tasks.md` and `notes.md`. This change only finishes what could not be checked there.

## Decisions

**1. The Linux backend stays as written until the compiler has seen it.** It follows a sequence that was run for real in WSL with a Python/ctypes prototype (start under `PTRACE_TRACEME`, stop at the first instruction, `int3` through `/proc/<pid>/mem`, `rip` back on the breakpoint, registers and frame reads, single step over the breakpoint, exit status and output). Fixes are whatever compiling and running it shows.

**2. The manual check is a checklist, not a test.** Driving VS Code or Antigravity automatically is not worth a dependency; the Node tests already exercise every provider against the real server. The check is done once by a person and recorded in `tasks.md`.

## Risks / Trade-offs

- A virus scanner may block the executables the tests build (Windows error 225); tests skip with a note, so a Linux run is the first full pass of `tests/debugger_linux.rs`.
- Linux needs `ptrace` permission (`kernel.yama.ptrace_scope`); the tracee is a child started with `PTRACE_TRACEME`, which the default setting allows.
