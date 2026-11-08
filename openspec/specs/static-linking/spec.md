# static-linking Specification

## Purpose
Define what a Zila Lang executable contains and what it loads at run time, so users know the cost of `use` and that packages are never fetched or loaded after the build.

## Requirements

### Requirement: Packages are compiled into the executable
Every package named by `use`, whether from the standard library or a dependency, SHALL be compiled together with the program into the single output executable. The executable SHALL NOT read `.zila` files, the package cache, `zila.toml` or `zila.lock` when it runs, and SHALL NOT load Zila Lang packages from shared libraries or plugins.

#### Scenario: Running without sources
- **WHEN** a program that uses `json` and a locked dependency is built and then run on a machine without the compiler, the cache or the project directory
- **THEN** it runs with the same behaviour

#### Scenario: Only FFI is dynamic
- **WHEN** a program declares `extern` functions of a system library
- **THEN** those are the only symbols resolved at run time, by the operating system loader

### Requirement: Loading a module has no effects
The top-level state of a package SHALL be initialised only when a reachable declaration needs it. A `use` whose package is never called SHALL NOT run package code, register operating system imports or add data to the executable.

#### Scenario: Unused import is inert
- **WHEN** `use ui` is added to a program that never calls it
- **THEN** the program makes no window-system import and behaves as without the `use`

### Requirement: Unreachable declarations are not emitted
For every library, a function, type helper or global that cannot be reached from `main`, an exported C entry, a `#[test]` being run or a `#[allocator]` SHALL NOT contribute code or data to the executable. Importing a library and using nothing from it SHALL NOT make the executable larger than the same program without the `use`.

#### Scenario: Unused import costs nothing
- **WHEN** `use tls` is added to a program that never calls it
- **THEN** the executable has the same size as the program without it

#### Scenario: Used function is kept
- **WHEN** the program calls `json.parse`
- **THEN** `parse` and everything it calls are in the executable

### Requirement: The link report
`zila <file> --explain-link` SHALL print, for each package in the build, its name, its kind (`program`, `std` or `package`) and the bytes of code and data it contributes, with the runtime, the strings and the generated helpers as separate lines, then the total of code and data, and SHALL NOT write the executable unless `-o` is also given. The numbers SHALL be measured on the assembled program, so they change when the program changes.

#### Scenario: Report
- **WHEN** a program uses `json` and one dependency and `--explain-link` is passed
- **THEN** one line per package with sizes is printed, and the total equals the sum of the lines

### Requirement: Platform layers expose the same surface
For each standard library with a Linux layer, every public function, type and global of `std/<name>.zila` SHALL exist in `std/<name>.linux.zila` with the same signature (record types compared by their public field names), except for the differences listed in `tests/data/std_parity_known.txt`: helpers that a layer declares `pub` only so another standard library can call them, and functions that exist for one platform only. A difference that is not in the list SHALL fail the test suite, and an entry that no longer applies SHALL fail it too, so the list can only shrink.

#### Scenario: A missing Linux function
- **WHEN** `std/fs.zila` gains a `pub` function that `std/fs.linux.zila` lacks and the list has no entry for it
- **THEN** the parity test fails naming the library and the function

#### Scenario: A fixed difference
- **WHEN** a listed difference is removed from the code but not from the list
- **THEN** the parity test fails asking to remove the entry
