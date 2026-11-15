# Changelog

All notable changes to Zila Lang are recorded here, newest first.
The format follows [Keep a Changelog](https://keepachangelog.com/) and versions follow [Semantic Versioning](https://semver.org/) (while below 1.0, minor versions may break compatibility).

The version lives in `Cargo.toml` and at the top of `README.md`. To release: move the entries from **Unreleased** under a new version heading, bump both places, and tag the commit (`git tag vX.Y.Z`).

## [Unreleased]

### Added
- Editor intelligence (OpenSpec change `add-editor-intelligence`): the language server (`zila --lsp`) now reports the errors and warnings of the compiler's checking on the expression they are about, with the unsaved text of every open file, and answers completion, hover, signature help, go to definition (also into the standard library), references, document and workspace symbols and rename; a text with a syntax error elsewhere still gets completion and hover, and checks wait for a quiet moment after typing.
- A debugger. `zila --debug` writes `<program>.zdbg` (a line table, functions, locals and type descriptors) next to the executable, which is byte-identical with and without the flag. `zila --dap` is a Debug Adapter Protocol server for Windows programs (breakpoints, continue, pause, step over/in/out, call stack, variables and values of every kind, the debug console, stopping on `abort` and on crashes); the Linux part (`ptrace`) is written but not yet built or run on Linux.
- The VS Code extension offers the new language features and a `zila` debugger (F5 on a `.zila` file); `docs/editors.html` describes them and the install command for VS Code (`code --install-extension`) and Antigravity (`antigravity-ide --install-extension`).
- The syntax tree carries source ranges (`ast::Span`); `parser::parse_partial` parses as far as it can; `codegen_x64::check_program` checks a program without emitting it.

### Changed
- `text.upper` and `text.lower` map letters of Latin (including Latin-1 Supplement, Latin Extended-A, Latin Extended Additional and fullwidth), Greek, Cyrillic and Armenian with simple one-to-one casing instead of ASCII only (`upper("ação")` gives `AÇÃO`); other scripts and characters with multi-character expansions like `ß` remain unchanged (OpenSpec change `add-unicode-case`).
- The command is now `zila` (before, its name ended in a suffix from the lexer-and-parser days); the VS Code extension starts `zila --lsp` by default (extension version 0.5.0).
- `zila.ebnf` and `zila-ffi.md` moved to `docs/`.
- The language's purpose is described more broadly in the README and the site: AI client and services, databases, web, IoT and mobile (the last two are targets), extensible through packages, open source.
- The README is about a third shorter and says openly that the language is designed with the help of AI under detailed human direction, and that the project most needs volunteers to refine it and, above all, to test and use it in practice. The diagrams moved to `docs/diagrams.md`.

### Fixed
- On Windows, `fs.stdin()` switches the console input code page to UTF-8 (`SetConsoleCP(65001)`), so accented characters typed interactively arrive intact without aborting (OpenSpec change `fix-console-input-encoding`).
- The README roadmap listed 15 changes as not started that were already built and archived, and a `rename-language` change that does not exist; it now lists only the real gaps. The test count is stated as "more than 1,000".
- The generated assembly is deterministic: two compiles of the same program no longer list the record and enum helper procedures in a different order.

### Added
- `zila repl` (also `zila -i`): an interactive session. Entries build on each other; a single expression prints its value; blocks continue on the next lines; a failed entry is discarded; `:help`, `:show`, `:reset`, `:quit`. Each entry re-runs the whole session and shows only its new output, so side effects of earlier entries repeat (OpenSpec change `add-repl`).
- `fs.stdin()`: a reader over standard input with the methods of a file reader (`read_line`, `read`, `read_all`, `at_end`, `close`), on Windows and Linux, from a pipe, a file or a terminal. It waits for the whole program while it reads. `fs.Reader` has a new field `std` (OpenSpec change `add-stdin-reader`).
- `zila run <file>` (the same as `--run`), `zila eval "<code>"` / `zila -e "<code>"` to run statements given inline, and `zila run -` / `zila eval -` to read the program from standard input. Errors in a snippet name the snippet's own lines (OpenSpec change `add-run-and-eval`).
- GitHub Actions workflow (`.github/workflows/ci.yml`): build, `cargo clippy -D warnings` and every test target except `async` on Windows and Linux, and the `async` tests as a separate job.
- `tests/docs.rs` checks that the README roadmap names only changes that are open in `openspec/changes/`, and says there are none exactly when there are none.

### Removed
- `docs/Zila.docx`, an old Word copy of the README that no longer matched it (the README is the one source; the file stays in the git history).

### Internal
- Clippy is clean with `-D warnings`: about 90 warnings fixed, and a few style lints that would make the code worse are allowed on the item with `#[allow]`.
- Declaration tables and generic instantiation results of the code generator live in `TypedProgram` (`src/codegen_x64/typed.rs`); `tools/asm-snapshot.py` compares the compiler's assembly before and after an internal change (`compare --labels` ignores how the compiler numbered its generated labels, for a change that stops it compiling something it throws away). Expressions are typed without emitting by `check_expr` (`src/codegen_x64/checker.rs`) into a typed tree (`TypedExpr`), and whole function bodies, `async` bodies included, by `check_function_body` (`src/codegen_x64/bodies.rs`) into a `TypedBody` kept in `TypedProgram::bodies` (it runs for every function in every build and adds about a tenth to a quarter to the compile time of a debug build); about six sevenths of the bodies in the examples are covered (closures, generic calls and `sql` blocks included), the rest, mostly C interop, stay `Opaque`. In every test build emission asserts that the type it gives an expression and every `let`/`var` is the one the checker found. Emission now consumes the typed tree where it can: calls of functions, methods, enum variants, the built-in functions and conversions and the methods of texts and chars are emitted to the target the checker resolved; the type of every `let`/`var` is read from the typed body; and the speculative compiles that typed a `sql` hole, the literal side of an operator and an argument of a generic call no longer run for any program of the test suite and the examples (they stay as a fallback for forms the checker does not cover, mostly C interop). For the rest, emission still works from the syntax and asserts in debug builds that the type it derives is the one the checker found. `tools/asm-snapshot.py compare --labels` showed the generated assembly unchanged apart from label numbers (OpenSpec change `add-typed-tree-emission`). The cache of checked packages is a later change.

## [0.1.0] - 2026-11-08

First tagged state of the language, as built between 2026-11-05 and 2026-11-08.

### Language
- Readable syntax with `end`-closed blocks, static types, errors as values (`T or error`, `try`), `defer`, `abort` (replaces panic), `pub` visibility.
- Generics and interfaces with explicit type arguments, derived `eq`/`ord`/`hash`, comptime reflection, `int`/`long` aliases, floating point, maps, sets and sorted collections.
- Reference-counted memory with copy-on-write, weak references, restricted reference escape, `#[no_alloc]` and `#[allocator]`, raw `asm` blocks.
- Async execution without function coloring, threads and `shared`.

### Compiler and toolchain
- Native x86-64 compiler for Windows (direct PE output) and Linux, with register allocation, inlining and a basic optimizer. The interpreter was removed.
- `zila fmt`, `zila test`, package manager, language server and VS Code extension, documentation site.
- Bare-metal OS (kernel written in Zila).

### Standard library and packages
- Text, bytes and char, lists, typed `format`, buffered I/O, JSON, UDP, async DNS, TLS, web, native UI on Windows and Linux.
- Packages: `json`, `crypto`, `db`, `postgres` (and other drivers), `ai` client, `mcp`, `rag`.
