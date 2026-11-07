## 1. Reference results

- [x] 1.1 Build every example and run every suite that builds Windows executables with the current `ml64`/`link` path, and save each program's output and exit code under a scratch folder; verify the file list matches the examples directory
- [x] 1.2 Add a developer-only environment variable (`BRASA_OLD_LINK`, set = the old path) that selects the old path during the migration, and verify that with it set the build still uses `ml64` and `link` (the new path being the default as soon as it existed, the variable is opt-in to the old one; removed in 5.2)

## 2. The assembler accepts the Windows output

- [x] 2.1 Add a test that generates the Windows assembly for each example and for `use ui`, `net`, `tls`, `task`, `float`, `format`, `sorted` and `fs` and assembles it with `asm_x86`, and verify it lists every unsupported instruction or form
- [x] 2.2 Teach the assembler each missing instruction or addressing form (and remove any MASM-only construct from the generated text instead where that is simpler), and verify the test from 2.1 passes for all programs
- [x] 2.3 Verify the bare-metal image test and the Linux suite still pass byte-for-byte unchanged

## 3. Import slots in the generated text

- [x] 3.1 Record `(dll, symbol)` pairs from `#[link]` for every extern the program uses, with `kernel32` for the runtime, and verify with a unit test on a program using `user32` and `ws2_32`
- [x] 3.2 Make the Windows text emit `GOT_<name> dq 0` slots and call through them instead of `extern X:proc`, keeping the old form behind the developer variable, and verify the old path still builds all examples
- [x] 3.3 Report an extern with no `#[link]` as a build error naming the symbol, and verify with a test

## 4. The PE writer

- [x] 4.1 Write the headers, sections and import directory for a program with only `kernel32`, and verify a hello world built this way runs and prints `hello`
- [x] 4.2 Add all DLLs and the IAT slots from the symbol table, and verify programs using sockets, files and `use ui` run
- [x] 4.3 Embed the manifest as a resource and choose the console or GUI subsystem from `uses_ui`, and verify `examples/ui.br` opens a themed, DPI-aware window with no console
- [x] 4.4 Set stack reserve and commit like the old image and verify the deep recursion and the 2000-task tests pass
- [x] 4.5 Make the compiler write the file only when the image is complete and map assembler errors to the Brasa file and line, and verify a program with a bad `asm` block fails with the file and line and leaves no temp directory

## 5. Remove the old path

- [x] 5.1 Compare outputs of every example and suite with the references from 1.1 and verify they are identical
- [x] 5.2 Delete `vcvars_candidates`, the batch driver, the temp directory and the developer variable, and verify the build succeeds with a `PATH` that has no Visual Studio tools
- [x] 5.3 Check what `#[link_src]` does today; make it a clear error on Windows and verify with a test
- [x] 5.4 Verify the size limits for `examples/hello.br` and `examples/ui.br` with a test

## 6. Documentation

- [x] 6.1 Remove the Visual Studio requirement from the README, `docs/getting-started.html` and `docs/native.html`, drop the finished roadmap item and update the status table, and verify the documentation tests pass
- [x] 6.2 Run the full suite (leaving `tests/async.rs` out of routine runs) and verify it passes
