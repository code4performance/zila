# Notes

## 1.1 What `add-typed-tree-emission` gives this change, and the gaps

Checked against the code at the commit that archived `add-typed-tree-emission`.

### What exists

- **Declaration tables** (`TypedProgram` in `src/codegen_x64/typed.rs`): records with field types and offsets, enums and variants, interfaces, function and method signatures, globals and constants, aliases, generic instances. Built before any body is emitted.
- **A typed body per function** (`TypedProgram::bodies`, keyed by symbol), built by `check_function_body` (`src/codegen_x64/bodies.rs`) for every function of every module the compiler generates, closures, `async` bodies and concrete generic instances included. Pruning of unreachable code happens after generation, so unused functions of imported modules are checked too. It holds the type of every `let`/`var` and typed statements; a statement it does not cover is `Opaque` (about 14% of bodies in the examples, mostly C interop).
- **`check_expr`** (`src/codegen_x64/checker.rs`): the type of an expression, call targets resolved (function, method, enum variant, generic instance, built-in, closure), arguments bound to parameters. Pure: no emission, no state change.
- **Scopes during the walk**: `self.fs.scopes` holds the locals visible at each point while a body is walked.
- The server already compiles a `main` package that parses, through `generate_asm`, and shows the compile error of the first failing line (`src/lsp/diagnostics.rs`). It never runs comptime code.

### Gaps (what the features need and the tree does not give)

1. **No source ranges.** `Expr`, `Stmt`, `Decl` carry only `line`; the typed nodes copy it. `Token` has `line`, `col` and a character `span`, but the parser drops them. Hover, definition, references, rename and signature help need the start and end of each name use and declaration. The parser must record spans and the typed nodes carry them.
2. **The checker reports no errors.** `check_expr` answers `None` for anything wrong or unknown. Compile errors still come only from emission (`R<String>` of the form `file:line: message`, the first one, no column, no range). Semantic diagnostics with ranges need the errors raised with a position, and more than the first.
3. **No entry point that stops before emission.** `check_function_body` runs inside `gen_func`; `generate_asm` does everything and returns assembly. The tree is dropped with the `Compiler`. The server needs a function that parses, loads the packages over an overlay of open documents, declares, checks all bodies and returns the tree, the declaration tables and the errors.
4. **Scope at a position is not kept.** Scopes are popped as the walk goes on. Completion needs the names visible at the cursor, and for a half-typed line, the receiver's type.
5. **No parser recovery.** `parse_source` returns the first error and no tree. Completion and hover in a file with an unrelated syntax error (spec scenario) need a partial tree.
6. **No per-node type record.** `TypedExpr` nodes are built and thrown away except those inside a complete typed statement; `Opaque` statements keep raw syntax. A position index that covers every name needs types for the forms the checker leaves opaque (C interop), or must answer "unknown" there.
7. **Generic templates and instances.** A template is checked once with opaque type parameters (no body check); each instance is walked separately. A position inside a generic body maps to several instances, or to none if never instantiated. The index must pick the template's own view.
8. **Doc comments are not attached to declarations.** The lexer keeps comments for the formatter only; hover needs the comment before a declaration.
9. **Other files.** Definition into `std` and other packages needs their text and positions; module files are parsed from disk (or the package cache) and carry lines only.
10. **Cost.** The whole program is checked on each run, standard library included; the budget and debounce in `design.md` apply. The body check adds roughly a tenth to a quarter to a debug-build compile.

### Consequence for the plan

Tasks 2.x and 3.1 start by adding spans to the AST and an entry point that returns the tree and the errors; the typed tree itself does not need to change shape.
