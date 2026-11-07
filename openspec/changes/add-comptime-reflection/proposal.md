## Why

Brasa is meant to be flexible and extensible: a library should be able to teach the language new tricks (serialize a record to JSON, derive equality, render an HTML template) without anyone changing the compiler. Today every such feature is either impossible or a compiler special case: `brasa.ebnf` even carries a one-off `html ... end` expression for `use web`. Copy-pasting per-type boilerplate (`to_json` for each record, `eq` for each record) is the alternative, and it does not scale to third-party packages.

The mechanism chosen is **restricted compile-time reflection**, not syntax-rewriting macros. The compiler exposes a small, read-only API over types, and a `comptime func` receives a type and **generates source text** (methods, helper functions) for it. Because there is no interpreter (`remove-interpreter`), a comptime function is compiled to a native program for the host, run, and its output is parsed and compiled in place. The comptime subset is deliberately small and checked by the compiler, so running a third-party package's generator at build time is deterministic and safe.

## What Changes

- **Type values at compile time**: a new built-in type `Type` that exists only at compile time, with a small reflection API: `name()`, `kind()` (`record`, `enum`, `alias`, `int`, `text`, `list`, `map`, `optional`, ...), `fields()` (name, type, attributes, line), `variants()` (name, payload fields, attributes), `element()`, `key()`, `value()` and `inner()` for containers, and `attrs()` (an attribute is a name and a list of literal arguments, e.g. `#[json("full_name")]`).
- **`comptime func`**: a function declared `comptime func derive_eq(t: Type) -> text` that returns generated source text. It is compiled for the host, run at build time, and may only use the comptime subset (text, lists, maps, sets, records, enums, loops; no I/O, no async or `spawn`, no raw pointers or `unsafe`, no `extern`, no `asm`, no global `var`). Violations are compile errors that name the offending call chain.
- **Derive attributes**: `#[derive(name, ...)]` on a type runs each named `#[derive] comptime func` of an imported package with the type and adds the returned declarations (typically methods with the type as receiver). Libraries define their own derives; `json` and `Eq/Ord/Hash` are built on this by other changes.
- **Types as arguments**: a function may declare a `comptime` parameter of type `Type` and be marked `#[generated(gen)]` with no body: `#[generated(parse_gen)] func parse(comptime t: Type, raw: text) -> t`. A call `json.parse(Config, raw)` passes the type as a compile-time value; the compiler runs `parse_gen(Config)` and compiles the returned text as the body of a specialization of `parse` for `Config` (one per distinct type, like generics).
- **Generic block form**: a name alone on a line, raw lines, then `end`. The parser recognises it for any library: the name must be a `#[block] comptime func` of an imported package, which receives the raw captured text and returns the source of an expression (or statements) that replaces the block. This **replaces** the special `html_expr` rule under `use web` in `brasa.ebnf`. The formatter keeps the content unchanged and the language server does not look inside it.
- **Safety and diagnostics**: a time limit (default 5 s, `--comptime-timeout`) and an output size limit (1 MiB) per run, a limit of 8 rounds of expansion, hygiene for generated names, and errors that point at the user's code (the derive attribute, the call, the block) and show the generated line that failed.
- **Cross-compilation**: comptime code always runs for the host, whatever `--target` says. Reflection data describes the program's types independently of the target.
- **Cache**: results are cached on disk keyed by a hash of the comptime function's transitive source, the reflected type description, the compiler version and the host.
- Docs, examples, tests and the grammar are updated in the same change.

## Capabilities

### New Capabilities

- `comptime-reflection`: the `Type` API, `comptime func`, derive attributes, type arguments, block forms, the comptime subset, execution model, limits, hygiene, cross-compilation and diagnostics.

### Modified Capabilities

<!-- None. Existing programs behave as before. Opaque block content in the formatter and the language server is stated inside the new capability. -->

## Impact

- **Lexer, parser, AST** (`src/lexer.rs`, `src/parser.rs`, `src/ast.rs`): keyword `comptime`; `comptime` parameters; `Type` in type position; raw capture for block forms, modelled on the `asm` block in `lexer.rs` (`in_asm`, `asm_line`); `FuncDecl` gains a comptime flag; the parser receives the set of block names found by a pre-scan of the file's `use` list.
- **Compiler** (`src/codegen_x64.rs`, new `src/comptime.rs`): subset checker, reflection data builder and serializer, host build-and-run driver, cache, splicing and re-parsing of generated source, source map for diagnostics, specialization of `#[generated]` functions. Uses `compile_to_native_with` for the host (the PE writer from `add-direct-pe-output` on Windows) and `compile_to_linux` on Linux.
- **Formatter and language server** (`src/fmt.rs`, `src/lsp.rs`): block content opaque; the server never runs comptime code.
- **CLI** (`src/cli.rs`, `src/main.rs`): `--comptime-timeout <seconds>`, `--no-comptime-cache`, `--comptime-dump`.
- **Standard library** (`std/`): a new `std/reflect.br` declaring the API; `std/text.br` and `std/list.br` are checked to be inside the comptime subset (and fixed if not). `os/` is untouched.
- **Grammar, docs, examples, tests**: `brasa.ebnf` (remove `html_expr`, add comptime and block rules), `docs/syntax.html`, `docs/native.html`, `docs/stdlib.html`, `README.md`, `examples/comptime.br`, new `tests/comptime.rs`, additions to `tests/parse.rs`, `tests/fmt.rs`, `tests/lsp.rs`.
- **Assumptions** (no clarification needed):
  - Generated code is **text**, not an AST: easy to write, easy to read (`--comptime-dump`) and it goes through the normal parser and checker.
  - Reflection is read-only and covers declared types only: it cannot inspect function bodies, other modules' private items or the file system.
  - Derive output may declare functions, methods, constants and helper types; it cannot modify the original declaration or add fields.
  - Arbitrary `comptime` blocks inside function bodies and constants computed by running code are out of scope.
