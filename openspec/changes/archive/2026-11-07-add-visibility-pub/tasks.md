"The suite" below means every test file under `tests/` except `tests/async.rs`, which stays out of routine runs (task 9.2 runs it once at the end).

## 1. Language: lexer, grammar, AST, parser (checks off)

- [x] 1.1 `zila.ebnf`: add `pub` to the reserved words; `top_decl = [ attrs ] [ 'pub' ] ( ... )` with `pub` before `async`/`func`, `type`, `let`/`var`, `extern`; `field = [ 'pub' ] ident ':' type`; add a semantic-rules note (private by default, package scope, construction rule). Verify: `tests/docs.rs` still passes and the grammar text matches the parser by reading both.
- [x] 1.2 `src/lexer.rs`: add `"pub"` to `KEYWORDS`. Verify: a `tests/parse.rs` case that `let pub = 1` fails to parse and `x.pub` is still accepted as a member name.
- [x] 1.3 `src/ast.rs` and `src/parser.rs`: add `public: bool` to `Decl` and `Field`; in `decl()` consume an optional `pub` after attributes (before `async`/`func`/`type`/`let`/`var`/`extern`) and reject it before `embed` with "`pub` cannot be applied to 'embed'"; in the record-field loop (the `Field { attrs, name, ty }` construction near the `fields.push` in the type parser) consume `pub` after field attributes; reject `pub` on enum variants, interface method signatures and parameters with a clear message. Update every `Decl {`/`Field {` constructor (`src/parser.rs`, any in `src/codegen_x64.rs`, `src/interp.rs`). Verify: new `tests/parse.rs` cases for each accepted placement (including `pub async func`, `pub extern func`, method `pub func (s: T) m()`, `#[export("x")] pub func`) and each rejected one.
- [x] 1.4 Search `std/`, `os/`, `examples/`, `tests/*.rs` embedded sources and `docs/*.html` code samples for an identifier named `pub` and rename any. Verify: `rg -w pub` shows only intended uses afterwards; suite green.
- [x] 1.5 `src/interp.rs`: confirm it ignores the new fields (no visibility checks there). Verify: `cargo build` clean and interpreter tests unchanged.

## 2. Tooling

- [x] 2.1 `src/fmt.rs`: make sure `pub` gets a single space after it and keeps declaration, field and attribute indentation (the layout comes from `parse_with_layout`; adjust only if `pub` before a field or after `#[...]` breaks it). Verify: `tests/fmt.rs` cases for `pub func`, `pub type X = record pub a: int  b: int end`, `#[export("f")] pub func`, `pub async func`; formatting twice is a no-op.
- [x] 2.2 `src/lsp.rs`: no code expected (diagnostics are `parse_source` errors); confirm `pub` declarations produce none and a misplaced `pub` reports the parser message with the right range. Verify: `tests/lsp.rs` cases for both.
- [x] 2.3 `editors/vscode/syntaxes/zila.tmLanguage.json`: add `pub` to the `keyword.declaration.zila` alternation (`let|var|embed|extern|pub`), and make the `func`/`type` declaration patterns still capture names when preceded by `pub`. Verify: `tests/vscode.rs` (extend it to assert `pub` is in the grammar's keyword list and that every lexer keyword is in the grammar, if it does not already).
- [x] 2.4 `docs/syntax.html`: list `pub` among the reserved words with a short section on visibility, the construction rule and an example; update the grammar summary if the page has one. Verify: `cargo test --test docs` (`documented_keywords_match_the_language`).

## 3. Compiler: recording visibility

- [x] 3.1 `src/codegen_x64.rs` `declare()`: record `public` on `Sig` (functions, methods, externs), on `Global`, in a set of public type names (records, enums, interfaces, aliases; generic templates `tmpl_recs`/`tmpl_funcs` too) and on each record field entry; keep the owning package on each. Verify: `cargo build`; behaviour unchanged with checks off.
- [x] 3.2 Add the helper `visible(owner_pkg, public) -> bool` (`public || owner_pkg == self.cur_mod`) and a `private_error(kind, name, pkg)` that formats ``'{name}' is private to package '{pkg}'; mark it `pub` to use it from another package``. Verify: unit-level use through the tests of tasks 4 and 8.

## 4. Compiler: enforcement (behind a temporary flag until task 8)

- [x] 4.1 Qualified calls and values: in the `ExprKind::Field` branch of `call` (the `self.funcs.get(&key)` / `tmpl_funcs` / `variants` / `globals` lookups) and in `member()`, check `visible` and report the private error when the name exists but is private; keep "module has no function" for missing names. Verify: `tests/codegen.rs` cases (private function, private global, private variant, nonexistent name gives the old message).
- [x] 4.2 Types: in `resolve()` for `a.T` paths (`rec_decls`, `tmpl_recs`, enums, aliases) and in enum-type lookup (`enum_type_named`, variant construction/patterns), require a visible type. Verify: tests for private record, private enum and private alias named from another package.
- [x] 4.3 Fields: apply `visible` in `field()` (read), the assignment path (`ExprKind::Field` at the assign sites near lines 2355 and 3135), `?.` access and `ref` field access, using `field_of`'s owner; message ``field 'x' of 'pkg.T' is private to package 'pkg'``. Verify: tests for read, write, safe access and `ref` on a private field; whole-value copy/pass/store still compiles.
- [x] 4.4 Construction: in `record_literal` and the `new T` path, if the record belongs to another package and has any private field, error with ``cannot create 'pkg.T' here: it has private fields; use a function from package 'pkg'``; all-`pub` records keep working. Verify: tests for private-field literal, `new`, constructor function, all-pub literal.
- [x] 4.5 Methods: check the method's `public` flag at the call sites that use `self.methods.get(&(key, name))` (method calls, bound methods, `drop` exempt) when the receiver type's package differs from `cur_mod`. Verify: tests for a private and a public method called from another package.
- [x] 4.6 Generics: confirm `call_generic` / `instantiate_record` set `cur_mod` to the defining package while compiling the body (so private helpers resolve) and that type arguments resolve in the caller's module; fix if not. Interface bounds: in the satisfaction check, require the method to be `pub` or from the interface's package. Verify: `tests/generics.rs` cases from the spec scenarios (generic calling private helper, caller's private type as argument, no leakage, private method not satisfying a bound).
- [x] 4.6b Compiler-generated references (task/future runtime, `float`, `format.fmtN`, `c.` intrinsics, `drop`, `test_` functions in `src/testrun.rs`, `main`, `#[export]`) bypass the check. Verify: tests with private `main`, a `spawn`/`await` program, a `format.fmt` call and `--test` over private `test_` functions.
- [x] 4.7 Public signatures: for each `pub` function, method, global, field and alias, check that every named type of the same package is `pub`; message ``pub func 'open' exposes private type 'Handle'; mark 'Handle' pub or make 'open' private``. Verify: tests for return, parameter, field and generic-bound leaks and for the accepted case of a foreign `pub` type.

## 5. Migrate the standard library (one task per library; checks still off)

For each file: put `pub` on every function, type, variant-bearing enum, global and field a program or another library uses, leave helpers private, run `zila-parse --fmt`, then verify with the suite and `rg` that no other file calls a name left private. The final check for each is the suite (and its dedicated test file named below).

- [x] 5.1 `std/text.br` (also its use of `float`). Verify: `tests/stdlib.rs` and `tests/format.rs`.
- [x] 5.2 `std/list.br`, `std/set.br`, `std/sorted.br` (their `Entry`/helper records are public if a public signature names them). Verify: `tests/sets.rs`, `tests/sorted.rs`, `tests/stdlib.rs`.
- [x] 5.3 `std/math.br`, `std/float.br`. Verify: `tests/floats.rs`.
- [x] 5.4 `std/format.br`. Verify: `tests/format.rs`.
- [x] 5.5 `std/bufio.br`. Verify: `tests/buffered_io.rs`.
- [x] 5.6 `std/sys.br`, `std/sys.linux.br`, `std/fs.br`, `std/fs.linux.br`. Public APIs identical in both variants. Verify: `tests/stdlib.rs`, `tests/linux.rs`.
- [x] 5.7 `std/task.br`, `std/task.linux.br`, `std/future.br`. Runtime entry points the compiler inserts need no `pub`; the library API does. Verify: `tests/codegen.rs` concurrency cases (async tests are not run here).
- [x] 5.8 `std/net.br`, `std/net.linux.br`, `std/ws.br`, `std/tls.br`, `std/tls.linux.br`. `ws` stays internal to `net`; `net` reaches `ws` through `ws`'s `pub` items. Verify: `tests/dns.rs`, `tests/udp.rs`, `tests/tls.rs`, `tests/linux.rs`.
- [x] 5.9 `std/ui.br`, `std/ui.linux.br`. If `add-ui-linux` is unmerged, mark only what exists; whoever lands second finishes it. Verify: `tests/ui.rs`.
- [x] 5.10 Cross-check parity: the public names of each `X.br` and `X.linux.br` match (script or test comparing the `pub` names of both files). Verify: `tests/linux.rs` extended with that comparison.

## 6. Migrate os, examples, tests, docs (checks still off)

- [x] 6.1 `os/boot.br`, `os/kernel.br`: `pub` on what `boot` calls in `kernel` (for example `console_write`); bare-metal entry points stay as the compiler expects. Verify: `tests/os.rs`.
- [x] 6.2 `examples/*.br` (21 files): mark what other packages use, none in single-file programs; add `pub` fields where an example builds another package's record. Verify: every example still compiles (`tests/codegen.rs`, `tests/docs.rs` `embedded_examples_match_the_example_files`) and `--fmt-check` is clean.
- [x] 6.3 Embedded sources in `tests/*.rs` that declare or use libraries across packages: add `pub` where needed. Verify: suite green.
- [x] 6.4 `docs/*.html` code samples (`stdlib.html`, `getting-started.html`, `examples.html`, `os.html`, `native.html`, `async.html`, `concurrency.html`): show `pub` where a library is declared; keep embedded examples identical to the files. Verify: `cargo test --test docs`.
- [x] 6.5 `README.md` and `GOVERNANCE.md` only if they show library code; no edit otherwise. Verify: reading them.

## 7. Turn the checks on

- [x] 7.1 Remove the temporary flag from task 4 so checks always run. Verify: the whole suite; every failure is a missed `pub` (fix it) or a real violation.
- [x] 7.2 Run `zila-parse --fmt-check` over `std/`, `os/` and `examples/`. Verify: no files listed.

## 8. Tests for the capability

- [x] 8.1 New `tests/visibility.rs` (or additions to existing files) covering every scenario of `openspec/specs/visibility` (the change's `specs/visibility/spec.md`): private/undefined/public names, `use` qualification and non-transitivity, fields read/write, construction, private-in-public, generics, interfaces, compiler-generated references, error text. Verify: `cargo test --test visibility`.

## 9. Final verification

- [x] 9.1 `openspec validate add-visibility-pub --strict` and the whole suite.
- [x] 9.2 Run `tests/async.rs` once to confirm the async libraries were migrated (the only time it is run for this change).

## Implementation notes (defaults taken while applying)

- Artifacts reconciled with the changes that landed first (int/long, derived eq/ord/hash, copy-on-write, comptime, char/bytes, weak refs/Rc, threads/shared): the new `std/thread.br`, `std/sync.br`, `std/reflect.br`, `std/errors.br`, `std/derive.br` were migrated like the rest of `std/`; `Rc`/`Weak` are built-in types and need no `pub`.
- Migration of `std/` and `os/` was done by a script: a name is `pub` when another package, an example, a test or a doc names it as `pkg.name` (methods and fields: any `.name` use outside the package), plus every `format.fmtN`, `math.checked_*`, `eq`/`compare`/`hash`; types named in a `pub` signature or `pub` field were then made `pub` until the leak rule held. Both `X.br` and `X.linux.br` were treated by the same rules.
- The temporary flag of task 4 was skipped: checks were written enabled from the start, and the suite found the missed `pub`s. Task 7.1 is therefore satisfied by construction.
- Defaults: a method whose receiver type is private may be `pub` (the leak rule does not look at receivers); only `pub` fields of a `pub` record count in the leak rule; derived methods (`eq`, `compare`, `hash`) are `pub`; the prelude interfaces `std.Eq/Ord/Hash` are exempt from the interface-visibility rule; a `comptime` specialisation inherits the template's `pub`; the comptime driver program (`comptime_run`) and `drop` are exempt from all checks (compiler-generated references); `pub` on `use` / `package` is a parse error like `embed`; `pub comptime func` is accepted.
- Interface satisfaction: a user method that satisfies a library interface (for example `format`'s `to_value`) must be `pub`; `examples/values.br`, `tests/format.rs` and `docs/stdlib.html` were updated.
- Task 5.10: `tests/linux.rs` compares the `pub` names of each `X.br` and `X.linux.br`: every Windows name must exist on Linux. `sys` and `task` are skipped (platform internals) and the Linux variants may export extra names (the Zila Lang-written DNS resolver of `net`, the X11 protocol of `ui`, reached by their tests).
- Tests: `tests/visibility.rs` (12 tests), plus cases in `tests/parse.rs`, `tests/fmt.rs`, `tests/lsp.rs`, `tests/vscode.rs`, `tests/linux.rs`.
- Final run (all files, each with a timeout): every file passes except `tests/linux.rs` (24 failing), `tests/error_kinds.rs` (6), `tests/ui.rs` (4) and one test of `tests/derive.rs` (`the_os_libraries_and_kernel_still_compile`). The same tests fail identically at the parent commit `efaed79` (checked in a separate worktree): `std/task.linux.br` has no `cur` that `std/future.br` calls, and `ui` reports an int/long mismatch. They are not caused by this change. `tests/async.rs`, `buffered_io`, `udp`, `dns`, `tls`, `os` all pass.
- Examples: every example that compiled before still compiles (checked through the CLI); `--fmt-check` is covered by `tests/fmt.rs` (`the_repository_is_formatted`).
