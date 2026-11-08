## Why

On Windows, Zila Lang still turns its generated assembly into an `.exe` by running Microsoft's `ml64` and `link`, found through Visual Studio's `vcvars64.bat`. That makes the main platform the least self-contained one: a Linux build needs only the compiler (it writes the ELF itself), while a Windows build needs a multi-gigabyte C++ toolchain. It contradicts the project's promise of a toolchain you can read, and it blocks running compiled code at build time (the planned `comptime` and `--run` after the interpreter is removed), since that needs a fast, dependency-free "compile to host executable" step.

## What Changes

- The compiler writes the Windows PE32+ executable itself, using the built-in assembler (`asm_x86.rs`) and a new PE writer, the same way `compile_to_linux` writes an ELF. No `ml64`, no `link`, no `vcvars64.bat`, no Visual Studio.
- The writer produces the import table (`kernel32` plus every library named by `#[link]` and by the standard library, e.g. `user32`, `gdi32`, `ws2_32`, `secur32`, `ntdll`; the standard library declares no `comctl32` or `crypt32` today, and the manifest alone gives window programs common controls 6), the entry point `zila_start`, the console or GUI subsystem, and the embedded application manifest (common controls 6, per-monitor DPI, UTF-8 code page) that `ui` programs need.
- The generated assembly stays the single source of truth for both targets. Anything it emits that the built-in assembler cannot yet accept (MASM-only directives, instructions, addressing forms) is either removed from the generated text or taught to the assembler.
- Build errors that came from `ml64` or `link` become compiler errors with the Zila Lang source position where possible.
- **BREAKING**: building on Windows no longer looks for Visual Studio, so the "could not find Visual Studio C++ build tools" error and the README requirement disappear. Programs that link C libraries through `#[link("lib")]` still need that DLL or its import information at run time, but no `.lib` file is needed to build. `#[link_src("path.c")]` (compiling C files) is not supported without a C compiler and is reported as an error until a separate change decides how to treat it.
- The README, the getting-started page and the roadmap are updated: the Visual Studio requirement is removed once this ships.

## Capabilities

### New Capabilities

- `windows-pe-output`: building Windows executables with only the compiler: the file format, imports, subsystem, manifest, errors, and equivalence with the old output.

### Modified Capabilities

- `linux-target`: the requirement that the default Windows build is unchanged is restated as "same program behavior and same output", since the bytes of the `.exe` change.

## Impact

- `src/codegen_x64.rs`: `compile_to_native_with`, `vcvars_candidates` and the `ml64`/`link` driver are replaced; a PE writer is added next to `elf_image`; the `MANIFEST` constant becomes an embedded resource. Parts of the generated assembly that use MASM-only syntax change.
- `src/asm_x86.rs`: gaps found while assembling the full standard library for Windows are filled (SSE for floats, calling-convention and unwind-free procedures, data directives).
- Tests: `tests/codegen.rs`, `async.rs`, `floats.rs`, `ui.rs`, `udp.rs`, `dns.rs`, `tls.rs` and the other suites that build executables must pass unchanged; a new test builds with `vcvars64.bat` and `ml64` deliberately absent from the environment.
- Docs: `README.md`, `docs/getting-started.html`, `docs/native.html`, the docs generator (`src/docs.rs`) and its tests.
