## Context

The compiler's unit of naming is the *module*, which today is a package name: `load` in `src/codegen_x64.rs` queues the main file and every `use`d std library, one `File` per package, and keeps `funcs`, `globals`, `variants`, `records` keyed by `(module, name)` or `"module.Name"`. Nothing records who may see a name. `is_module`, the qualified-call branch of `call` (the `module '{}' has no function '{}'` path), `member` (globals and variants), `resolve` (types written `a.T`), `field_of` and `record_literal` all look names up without a check.

Methods live in a table keyed by receiver type, not by package. A package is one file in practice today; `add-package-manager` introduces multi-file packages, so the rule is stated per `package` name and the implementation compares package names, never files.

## Decisions

1. **Private by default, scope = package.** A name without `pub` is visible in every file declaring the same `package` and nowhere else. There is no third level: package-private is the default and `pub` the only modifier.
2. **Placement.** `pub` comes after attributes and before everything else: `[attrs] ['pub'] ['async'] 'func'`, `['pub'] 'type'`, `['pub'] ('let' | 'var')`, `['pub'] 'extern' ... 'func'`, and `['pub'] ident ':' type` for a record field (field attributes stay before `pub`). A method is `pub func (r: T) name(...)`. `pub embed`, `pub` on `package` / `use`, on enum variants, on interface method signatures and on parameters are parse errors ("`pub` cannot be applied to ..."). `pub` is a reserved word (lexer `KEYWORDS`), so it can no longer be an identifier; names after `.` may still be keywords, as for `use`.
3. **Types.** `pub type` exports the type name and, for an enum, all its variants. For an interface, `pub type Printable = interface ... end` exports the interface; its method signatures carry no `pub`. A non-`pub` type is usable only inside its package, together with its fields and variants. An alias is exported on its own (`pub type Id = int`).
4. **Fields.** Fields are private unless written `pub`. Reading or writing a private field (`p.x`, `p.x = 1`, `p?.x`, through `ref`) is allowed only inside the package. Copying, passing, returning or storing a whole value is always allowed: privacy hides names, not values. The compiler's own use of fields (`drop`, copies, layout) is unaffected.
5. **Construction rule.** A record literal `T { ... }` / `pkg.T { ... }` (and `new T`) outside the declaring package is legal only if the type has no private field; otherwise it is an error: ``cannot create 'text.Builder' here: it has private fields; use a function from package 'text'``. There are no partial literals. A library that wants outside construction either makes every field `pub` or exports a constructor (`pub func new_builder() -> Builder`). Pattern: records with invariants keep fields private and export constructors; plain data records use `pub` fields.
6. **`use` and names.** `use text` makes the public names of `text` reachable only as `text.name` (types `text.Type`, variants through the type). It never imports names unqualified, and never re-exports what `text` itself uses (`use float` inside `text` gives the program nothing). A name that exists but is private reports the private error; a name that does not exist keeps today's message.
7. **No leaking private types.** A `pub` function, method, global, field or alias whose signature (parameter, return, declared type, field type, interface bound) names a private type of its own package is a compile error: ``pub func 'open' exposes private type 'Handle'; mark 'Handle' pub or make 'open' private``. Types from other packages are fine, since only `pub` ones can be named.
8. **Error messages.** All are compile errors with the use-site line:
   - ``'index_from' is private to package 'text'; mark it `pub` to use it from another package`` (functions, globals, types, variants, methods);
   - ``field 'len' of 'mem.Buffer' is private to package 'mem'``;
   - the construction error of decision 5 and the signature error of decision 7;
   - parse errors for misplaced `pub` (decision 2).
9. **Generics keep the definer's rights.** `call_generic` and `instantiate_record` compile a generic body against the module of its declaration, so checks use the package of the code being compiled (`cur_mod`): a monomorphized body may call the defining package's private helpers and read its private fields even when instantiated from another package. Type arguments written or inferred in the caller resolve in the caller's package, so a generic from `list` may be instantiated with a private type of the caller. Interface bounds: a type satisfies an interface if each required method is visible from the interface's package (a `pub` method, or one of the same package); a private method of another package does not satisfy it.
10. **Compiler-generated references are exempt.** Calls the compiler inserts do not need `pub`: the task and future runtime behind `spawn` / `await` / channels, `float` helpers, the `format.fmtN` renaming, `c.` intrinsics, implicit `drop`, the test runner calling `test_` functions (`src/testrun.rs`), `main`, and `#[export("name")]` functions. `src/interp.rs` parses `pub` and ignores it; enforcement lives in the native code generator.
11. **Implementation shape.** The parser sets `public: bool` on `Decl` and `Field`. `declare()` records a `public` flag on `Sig` (functions, methods, externs), `Global`, the type tables and each record field. One helper `visible(owner_pkg, public)` (true when `public || owner_pkg == self.cur_mod`) is called from the qualified-call branch, `member`, `resolve`, field access and assignment, `record_literal`, `new`, method lookup, and the interface-satisfaction check. Names of the current package never pay for a check.

## Depends on

- `add-direct-pe-output` (already written): independent; only the order of final test runs matters.
- `add-package-manager`: introduces multi-file and external packages; `visible()` must compare package names so the rule holds there. Until then each package is one file.
- `remove-interpreter`: if it lands first, the `src/interp.rs` part of this change disappears.
- Every change that adds or edits std or example code (`add-json`, `add-web`, `add-threads-and-shared`, `add-no-alloc-and-allocator`, `add-text-bytes-and-char`, `add-weak-refs`, `add-int-long-aliases`, `add-error-kind`, `remove-async-coloring`, `remove-panic-add-abort`, `add-copy-on-write`, `restrict-ref-escape`): whichever lands later writes its Zila Lang code with `pub`; whichever lands earlier gets migrated by tasks 5 to 7 here.
- `add-comptime-reflection` and `add-derived-eq-ord-hash`: reflection does not bypass privacy (out of scope here); derivations are generated inside the type's own package, so they may use private fields.
- Not touched: `add-ui-linux` (work in progress by someone else). It edits `std/ui.linux.br`; whoever merges second applies `pub` there (task 5.9).

## Risks / Trade-offs

- **Reserved word.** `pub` stops being an identifier. A search of `std/`, `os/` and `examples/` finds no use; tests and docs are rechecked in task 1.4.
- **Large mechanical diff.** About 9,500 lines of std plus many embedded test programs gain `pub`. A missing one yields a clear error naming the private name, so the suite finds them. Each library is its own task with a full-suite check.
- **Private-in-public rule** forces exporting helper types that public signatures already use. Accepted: it keeps the public surface honest.
- **No partial literals**: a record with one private field cannot be built outside its package. Accepted; constructors are the pattern.

## Migration Plan

1. Land lexer, grammar, AST, parser, docs keyword list and tooling with the checks off (everything compiles as before).
2. Mark `pub` across std, os, examples, tests and docs, library by library, suite green after each.
3. Turn the checks on, fix what the suite reports, add the negative tests.

Pre-1.0 policy (`GOVERNANCE.md`) allows the break; no deprecation period.
