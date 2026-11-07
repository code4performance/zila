## Context

See proposal.md for the motivation. Today `compile_to_native_with` generates MASM-flavoured text, writes it with a manifest to a temporary directory, finds a Visual Studio through `vswhere` and `vcvars64.bat`, and runs `ml64` then `link` through a batch file. The Linux path already does the equivalent without tools: `generate_asm_for` produces the text, `asm_x86::assemble` turns it into bytes at a chosen base address, and `elf_image` / `elf_dynamic_head` wrap the bytes with headers and an import mechanism (`GOT_<name> dq 0` slots that the loader fills; the writer learns the slots from the assembler's symbol table).

Two facts make Windows simpler than it looks: the assembler already accepts the text the Windows backend emits (the bare-metal and Linux builds use the same instruction and directive dialect), and every `extern` in the standard library already carries a `#[link("dll")]` attribute, so the DLL of each imported symbol is known to the compiler. What `link` provides today beyond that, and so must be reproduced, is the PE container: DOS stub, PE and optional headers, sections, the import directory with the IAT, the entry point, the subsystem, alignment, and the embedded manifest resource.

## Goals / Non-Goals

**Goals:**
- A PE32+ writer in the compiler, modelled on `elf_image`, that needs nothing outside the repository.
- Windows and Linux builds share every step up to "assembled bytes plus symbol table".
- The same program behavior as the `ml64`/`link` build, validated by the existing test suites.
- No change to the language, the standard library's API or the bare-metal build.

**Non-Goals:**
- Debug information, `.pdb` files, code signing, relocations/ASLR for the image (see decision 4) and import by ordinal.
- Compiling C sources (`#[link_src]`) or linking `.lib` or `.obj` files.
- ARM64 or 32-bit Windows.
- Changing the text the compiler emits beyond what the assembler needs.

## Decisions

**1. Import slots like the Linux backend.** The generated assembly stops saying `extern X:proc` and `call X` (which relies on `link` creating import thunks). It emits `GOT_X dq 0` slots in the data and calls through them (`call qword ptr GOT_X`), exactly as the Linux target does. The PE writer builds an import directory whose IAT entries are those slots, so the loader writes the addresses in place. Alternative: emit `jmp [slot]` thunks as `link` does. Rejected: an extra jump per call and more code for no benefit.

**2. Map symbols to DLLs through the existing `#[link]` data.** Each extern already knows its library through its attribute; the compiler records `(dll, symbol)` pairs instead of the flat list of libraries, defaulting to `kernel32` for the runtime's own calls. The import directory has one descriptor per DLL in first-use order, so output is deterministic. A symbol with no `#[link]` is reported as a build error naming the symbol (today `link` would complain it is unresolved). Alternative: a table inside the compiler listing every Windows function by DLL. Rejected: it would duplicate the standard library's own declarations and go stale.

**3. One image, three sections.** `.text` (code, read and execute), `.data` (data and import slots, read and write), and `.idata` plus `.rsrc` (imports and the manifest, read only), with 4 KiB virtual alignment and 512-byte file alignment, which keeps a hello world within the size limit. The assembler runs once with the image base and the virtual address of `.text` as its origin; see decision 9 for how the one assembled blob is split into the two sections.

**4. A fixed image base, no relocations.** The image base is a constant (as the Linux backend does), the header says no ASLR (`DYNAMIC_BASE` off) and the file has no `.reloc` section. The code is already position-independent for its own data (RIP-relative), but absolute addresses appear in data tables and are not worth relocating for a first version. Alternative: emit `.reloc` and enable ASLR. Deferred: it can be added later without changing the language, and it would only matter for hardening.

**5. The manifest becomes a resource compiled by the compiler.** The existing `MANIFEST` text is embedded as an `RT_MANIFEST` resource (ID 1) in `.rsrc`. Only the three-level resource directory tree is needed for that. GUI programs get the subsystem value for Windows, console programs the console value, chosen from `uses_ui` as now.

**6. Entry stays `brasa_start`.** The PE entry point is the symbol `brasa_start`, which already calls `SetConsoleOutputCP(65001)` and sets the stack up. No C runtime is involved, as today.

**7. Remove, don't keep, the old path.** `vcvars_candidates`, the batch file and the temp directory are deleted rather than kept as a fallback. Keeping both would mean testing both forever and would leave "needs Visual Studio" as a documented option. The old output remains reachable through git history if needed to compare behavior during development (task 1.1 records reference outputs before any change).

**8. Errors come from the compiler.** `asm_x86` errors already carry the assembly line number; the writer maps it back to the Brasa line using the markers the code generator leaves, and failures leave no temporary files because nothing is written until the image is complete.

**9. Facts found while implementing.**
- The assembler produces one flat blob. The writer puts `rt_text_end:`, `align 4096` and `rt_data_start:` where the text says `.data`, assembles once at `image base + 0x1000`, and cuts the blob into `.text` and `.data` at those symbols (no two-pass header trick is needed: the headers have a fixed size). The assembler now reports source line numbers (blank and comment lines counted) so errors map to the generated text, and the code generator leaves a `; asm file.br:line` marker before each inline `asm` block.
- The loader does not bind an import whose address slot starts as zero, even when the lookup table (`OriginalFirstThunk`) is present: the slots must start as copies of the lookup table (hint/name RVAs), as `link` writes them. The writer patches them into the data after assembly.
- The image base is `0x140000000` (the usual one for 64-bit programs); `0x400000` also loads but nothing needs it.
- The `ucrt` import library name stands for `ucrtbase.dll`; every other library name is used as the DLL name.
- `#[link]` names the library of the externs that follow it up to the next `#[link]` in the same file (the standard library relies on this), so the symbol-to-library map is built in that order.
- The manifest is embedded in every executable (as `link /manifest:embed` did), not only in window programs; the subsystem alone depends on `uses_ui`.
- `ml64` and `link` were only reachable through `vcvars64.bat`; the old output of every example was recorded before the change (task 1.1) and the example outputs are identical afterwards. The generated assembly text differs (slots instead of `extern`), so the two tests that looked for `extern X:proc` in the text now look for `GOT_X dq 0`.
- Output is not byte-for-byte reproducible between runs yet, because the code generator walks some `HashMap`s; the import table is deterministic.

## Risks / Trade-offs

- [The assembler lacks an instruction or form the Windows output uses (SSE for floats, `movsd` variants, `cqo`, string operations)] → Build the entire standard library and every example for Windows through the assembler first (task 2), as a test that only assembles, before any PE code exists. Gaps found are fixed in the assembler, in small separate commits, with a test each.
- [A subtle difference in generated code between `ml64` and the assembler (encoding choices, alignment, `proc` frames)] → The existing suites are the oracle. The assembler always uses the long displacement forms, so code gets slightly bigger than `ml64`'s; the size limit in the spec has room for it.
- [Stack probing: Windows requires touching guard pages in order when a frame is larger than a page] → `ml64`/`link` add nothing here, but the generated code already handles it for large frames today; the PE writer must reserve and commit the same stack size as the old image (the `link` default of 1 MiB reserve, 4 KiB commit) in the optional header. Test: a recursion depth test that passes today must pass.
- [Manifest or DPI behavior differs slightly when embedded by hand] → The `ui` test suite and a manual check of `examples/ui.br` for DPI scaling and themed controls.
- [Antivirus heuristics distrust small hand-written PEs] → Use standard section names, valid checksums and alignment, and a populated import table; nothing is packed or obfuscated.
- [`#[link_src]` is documented but appears to have no implementation in the compiler] → Task 5 confirms; if it is already ignored, the spec requirement turns it into a clear error rather than silent breakage.

## Migration Plan

All Brasa code lives in this repository, so there is nothing external to migrate. The change lands in this order, each step keeping the suite green: (1) record reference results, (2) make the assembler accept the full Windows output, (3) add import slots to the Windows text, (4) write the PE, (5) delete the `ml64`/`link` path, (6) update the documentation and README. Until step 5 the old path stays selectable by an environment variable used only by developers, to compare outputs. Rollback before step 5 is turning that variable on; after step 5 it is reverting the commit.

## Open Questions

- Whether to set a larger stack reserve than `link`'s default, since tasks use their own stacks. It does not change specs or tasks and can be tuned later from measurements.
