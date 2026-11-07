# windows-pe-output Specification

## Purpose
Defines how Brasa builds Windows executables using only the compiler, so that building on Windows needs no Visual Studio, `ml64`, `link` or other external tool, just as the Linux target needs none.

## Requirements

### Requirement: Building a Windows executable needs only the compiler
`brasa-parse prog.br -o prog.exe` (and `--target windows`) SHALL write a 64-bit Windows executable without running any other program and without reading any Visual Studio, Windows SDK or other toolchain file. It SHALL succeed on a machine where none of `ml64`, `link`, `vcvars64.bat` and `vswhere` exist.

#### Scenario: A machine with no Visual Studio
- **WHEN** a hello-world program is built on Windows with no Visual Studio installed and no `ml64` or `link` on the path
- **THEN** the build succeeds and the executable prints `hello` and exits with status 0

#### Scenario: No tool is started
- **WHEN** a program is built
- **THEN** the compiler starts no child process

### Requirement: Same behavior as before
A program built with the compiler's own writer SHALL behave exactly as it did when built with `ml64` and `link`: the same output on every example and test program, the same exit codes (a `main` that returns an error exits with 1), the same results from `live_objects()`, and the same behavior of tasks, `async`, files, TCP, UDP, DNS, TLS, floats, and C calls.

#### Scenario: Examples agree
- **WHEN** the example programs are built and run before and after this change
- **THEN** their output is identical

#### Scenario: The existing suites
- **WHEN** the test suites that build and run Windows executables run
- **THEN** all pass without changes to their expectations

### Requirement: Imports come from the program, not from a linker
The executable SHALL import exactly the functions that the program and the libraries it uses declare with `extern`, each from the library named by its `#[link("lib")]` attribute (`kernel32` for the runtime itself). A program SHALL NOT import a library none of its functions uses. A library name is written without extension and resolved by the Windows loader at program start; an import the system cannot provide SHALL make the program fail at start-up, as any Windows program does, and SHALL NOT be a build error.

#### Scenario: A console program
- **WHEN** a program that uses no window, socket or TLS function is built
- **THEN** its import table lists only `kernel32` (and `ntdll` if `use task` is present)

#### Scenario: A window program
- **WHEN** a program that does `use ui` is built
- **THEN** the executable imports `user32`, `gdi32` and `kernel32` (the standard library declares no other library for windows), has the common-controls 6 manifest, and opens a window when run

#### Scenario: An unknown function in a known library
- **WHEN** a program declares `#[link("kernel32")] extern func NoSuchFunction()` and calls it
- **THEN** the build succeeds and the program fails when it starts, with the loader's error

### Requirement: Console and window programs
A program that does not use `ui` SHALL be built for the console subsystem and one that does SHALL be built for the Windows (GUI) subsystem. UTF-8 SHALL be the process code page, per-monitor DPI awareness and common controls version 6 SHALL be enabled for window programs, as before.

#### Scenario: Subsystem
- **WHEN** one program with `use ui` and one without are built
- **THEN** the first opens no console window and the second runs in the console

#### Scenario: Multibyte output
- **WHEN** a program prints `ação` to a console
- **THEN** the bytes written are UTF-8 and the text shows correctly

### Requirement: Size
A hello-world executable SHALL be no larger than 8 KB, and the window example (`examples/ui.br`: a window, a text box, a button and a list, with the whole `ui` library) SHALL be no larger than 64 KB and no larger than the same program was when built with `ml64` and `link`.

#### Scenario: Small executables
- **WHEN** `examples/hello.br` and `examples/ui.br` are built
- **THEN** the sizes are within the limits

### Requirement: Failures are reported by the compiler
Anything that stops a build SHALL be reported by the compiler in its own words, with the Brasa file and line where it can. A build SHALL NOT fail with text from `ml64` or `link`, and SHALL NOT leave intermediate files behind.

#### Scenario: A bad program
- **WHEN** a program that cannot be assembled (for example inline `asm` with an unknown instruction) is built
- **THEN** the compiler names the file and line of the `asm` block and the instruction, and no temporary directory remains

### Requirement: C source files need a C compiler
`#[link_src("file.c")]` SHALL be rejected on Windows with a message that says the compiler no longer builds C sources there. `#[link("lib")]` to a DLL SHALL keep working with no `.lib` file present.

#### Scenario: Linking a DLL
- **WHEN** a program declares `#[link("user32")] extern "system" func MessageBeep(kind: u32) -> i32` and calls it
- **THEN** it builds without any import library and runs

#### Scenario: A C source
- **WHEN** a program uses `#[link_src("helper.c")]`
- **THEN** the build fails with a message about C sources
