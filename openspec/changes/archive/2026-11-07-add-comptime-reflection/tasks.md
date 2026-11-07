## 1. Syntax

- [x] 1.1 Add the keyword `comptime` and `Type` to the lexer and parser: `comptime func` declarations and `comptime` parameters (`FuncDecl` in `src/ast.rs` gets a flag and per-parameter flag; `Parser` in `src/parser.rs`); verify with parse tests in `tests/parse.rs` for valid forms and for `comptime` on a non-function
- [x] 1.2 Parse the attributes `derive`, `generated(name)` and `block` and allow a body-less `#[generated]` function (parser currently requires a body for non-extern functions); verify with parse tests, including a missing generator name
- [x] 1.3 Add the block form: pre-scan the `use` lines for `#[block]` names, raw capture in `src/lexer.rs` modelled on `in_asm`/`asm_line`, `Tok::Block`, `Expr::Block` and the statement form in the parser; verify with tests for an expression block, a statement block, an unknown name (ordinary identifier) and a block whose content is not valid Brasa
- [x] 1.4 Update `brasa.ebnf`: remove `html_expr` and its section, add `comptime`, `Type`, `block_expr`; verify by grep that `html_expr` is gone and `cargo test --test parse` passes

## 2. Formatter and language server

- [x] 2.1 Make `src/fmt.rs` keep block content byte for byte and re-indent only the name and `end`, running the same pre-scan; verify with `tests/fmt.rs` cases (idempotent, content untouched, unresolved name formats as ordinary code)
- [x] 2.2 Make `src/lsp.rs` never parse block content and never run comptime code; verify with a `tests/lsp.rs` case where the block content is invalid Brasa and no diagnostics are published

## 3. Reflection data and the subset check

- [x] 3.1 Add `std/reflect.br` (`Field`, `Variant`, `Attribute`, `fresh`, `fail`, `target`) and register it in the standard library table in `src/codegen_x64.rs` (`std_source`); verify it compiles with `cargo test --test stdlib`
- [x] 3.2 Implement the `Type` queries and the serialization of a type description (and every type reachable from it) in a new `src/comptime.rs`, plus the reader in the driver program; verify with unit tests that round-trip a record, an enum, a list of records and a type with attributes
- [x] 3.3 Implement the subset checker (reachability over the call graph, with the chain in the message) and the rejection of `Type` outside comptime and of runtime calls to comptime functions; verify with one compile-error test per construct: `extern`, `asm`, `spawn`, `async`/`await`, channel, raw pointer, `unsafe`, global `var`, `fs.read` through a helper, through a closure and through a generic instance, and with a positive test over `text`, `list` and `format` helpers

## 4. Running generators

- [x] 4.1 Build the driver program and the host executable for a batch (Windows host: the PE path from `add-direct-pe-output`; Linux host: `compile_to_linux`), run it as a child with piped input and output, the timeout, the output cap and the memory limit, and parse `ok`/`fail`; verify with a test generator that echoes its type, an endless generator (timeout error), an over-sized output, and a crashing generator
- [x] 4.2 Implement the cache (key hashing, store, hit, miss, `--no-comptime-cache`) and the flags `--comptime-timeout` and `--comptime-dump` in `src/cli.rs` and `src/main.rs`; verify with tests that count child launches for: second identical build (none), edited generator body, edited helper called by the generator, added field, different compiler version string
- [x] 4.3 Verify cross-compilation: on the host, `--target linux` (Windows host) and `--target windows` (Linux host, where supported) run the generator for the host and produce the right target output; verify with a test in `tests/comptime.rs` skipped where the host cannot run the other target's output

## 5. Using the output

- [x] 5.1 Implement `#[derive(...)]`: lookup in imported packages (qualified names, unknown and ambiguous errors), expansion before `declare` in `Codegen::load`, synthetic module with the generator's package and the type's module in scope, method attachment, collision errors; verify with tests for a method derive, two derives on one type, unknown name, ambiguity, derive on an enum, and a user method colliding with a derived one
- [x] 5.2 Implement hygiene: `reflect.fresh`, rejection of user declarations starting with `__`, synthetic module isolation; verify with the helper-without-capture test and a test using `fresh` twice
- [x] 5.3 Implement `#[generated]` specializations keyed by `(function, type)` with `comptime` parameters erased from the instance signature and `t` bound as an alias; verify that one generator run happens per type, that the result type `t` works, that a non-type argument is rejected, and that the specializations leak nothing under `live_objects()`
- [x] 5.4 Implement block expansion (expression and statement forms, scope of the block's position, `reflect.fresh` for locals); verify with the `shout` scenario, a block that reads a user variable and a block in statement position
- [x] 5.5 Enforce the expansion limit (8 rounds) with the chain of generators in the message; verify with a self-deriving type

## 6. Diagnostics

- [x] 6.1 Add the source map and report errors in generated code at the use site with the generator name and the generated line; implement `reflect.fail` mapping to the user's field, variant or type line; verify with tests asserting the exact wording for: generator refusal on a field line, a type error in generated text, a parse error in generated text, a failed subset check (names the chain), and a limit error naming generator and use site

## 7. Standard library, examples, docs

- [x] 7.1 Check `std/text.br`, `std/list.br`, `std/format.br` and `std/math.br` against the subset checker and fix anything impure they reach; verify with a comptime test importing each
- [x] 7.2 Add `examples/comptime.br` (a `describe` derive, a type-indexed `#[generated]` function and a small block) as a package-local generator pair of files, and its section on `docs/examples.html` through `brasa-parse --docs`; verify it builds and prints the documented output
- [x] 7.3 Update `docs/syntax.html` (comptime, `Type`, derive, generated, blocks, limits, cross-compilation, trust model), `docs/native.html` (the comptime pipeline and cache), `docs/stdlib.html` (`reflect`), and `README.md` (status table and roadmap); verify `cargo test` passes, including the docs tests, with `tests/async.rs` left out of routine runs
- [x] 7.4 Final check: no remaining mention of the `html ... end` special syntax in `brasa.ebnf`, docs or README (grep), `os/` and the rest of `examples/` still build, and `openspec validate add-comptime-reflection --strict` passes

## Notes

Defaults taken while implementing (no clarification was possible):

- **Derived `eq`/`compare`/`hash` stay in the compiler.** Nothing in this change's design, tasks or spec asks to move them to comptime, so `src/derive_ops.rs` is unchanged. Moving them later means writing `#[derive]` generators over `reflect`.
- **Packages.** The compiler still has no package manager, so "imported package" means the standard library, packages registered with `comptime::register_package`, and the files next to the source file that a `use` names (`<dir>/<name>.br`, registered by the command line, formatter, test runner and language server). A program's own package may hold generators too.
- **Splicing.** Generated declarations are spliced into the module of the type instead of a synthetic module `<module>.__gen<n>`: module names with dots are not safe in the compiler. Hygiene is kept by renaming the generated top-level functions, types and constants to `__g<n>_<name>`; methods attach to their receiver. Specializations of a `#[generated]` function go to the module that declares it.
- **Driver input.** The type tables are written in the line-oriented wire format (cache key, `--comptime-dump`, tests) and the driver program rebuilds them with `reflect.add_type(...)` calls generated by the compiler, instead of a Brasa parser reading standard input. `reflect.fail` prints a `fail` record and aborts; the first failure ends the batch.
- **Variant payloads** may be named (`Circle(r: float)`); positional ones are called `_0`, `_1`. Attributes may stand on the line before a field or variant.
- **Type arguments.** A `comptime` argument is a type name (`Config`, `pkg.T`, a built-in), or `f<T>(...)` with an explicit type argument; a bare `[]User` argument is not an expression.
- **Cache.** Directory: `BRASA_COMPTIME_CACHE`, else `brasa-comptime` in the system temporary directory. The key also includes the target name (`reflect.target()` is an input). Tests count runs with `comptime::launch_log()`.
- **Language server** does not compile a program that asks for code generation (it would run generators); it reports parse errors only for such files.
- **Block names** cannot be qualified (`pkg.name` is not a block); the name must be unambiguous among the imports.
- **Not run (user rule):** `tests/async.rs`, `buffered_io`, `udp`, `dns`, `tls`, `linux`, `os`, `ui`, `error_kinds` were only compiled (`cargo test --no-run`). The Linux-host path of the driver (ELF build, `setrlimit`) was not run; on this Windows host `--target linux` was checked by building the ELF (header check) and by the generator being told `linux`.
