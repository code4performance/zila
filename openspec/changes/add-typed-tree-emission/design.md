## Context

Currently, `Compiler` resolves types, instantiates generics, checks borrow/visibility rules, and emits assembly text in a single pass (`Compiler::expr`, `Compiler::stmt`). Declarations already live in `TypedProgram` (`src/codegen_x64/typed.rs`), and `type_of` (`src/codegen_x64/checker.rs`) provides a read-only typing check that is asserted against emission in debug builds. However, `type_of` only covers a small subset of expressions, and the emission pass still carries speculative compilation rollbacks (`Snapshot`), re-calculates types during codegen, and contains numerous defensive `unreachable!()` branches.

## Goals / Non-Goals

**Goals:**
- Separate type checking/resolution of function bodies completely from machine code emission.
- Introduce an explicit typed intermediate representation (`TypedExpr`, `TypedStmt`, `TypedBody`) where each expression has an unambiguous `Ty` and resolved call targets (callee symbol, method dispatch, or generic instance).
- Make `Compiler::emit_*` consume the typed tree directly, without querying type inference or performing speculative rollbacks.
- Keep the generated assembly byte-identical to previous builds across all examples and targets (`--asm`, `--target linux`, `--no-opt`), verified by `tools/asm-snapshot.py`.

**Non-Goals:**
- Package caching to disk (`$ZILA_HOME/check/`): that is a separate follow-up change once the typed representation is complete and validated.
- Changes to language syntax, standard library APIs, or executable binary behavior.
- High-level optimization passes (SSA/LLVM-style optimization): the existing peephole and reachability pruning remain unchanged.

## Decisions

1. **Step-by-step expansion of the typed tree alongside emission**:
   Rather than a monolithic rewrite, expand `checker.rs` expression form by expression form. As each form is covered, `type_of` coverage increases until 100% of expressions and statements in the test suite and standard library are typed ahead of emission.
   *Alternative considered:* Rewriting the entire codegen at once. *Rejected:* High risk of subtle behavioral regressions across the 1,000+ tests and native Windows/Linux backends.

2. **Explicit typed AST nodes vs side-table**:
   Represent typed bodies using typed nodes (`TypedExpr` holding `(TypedExprKind, Ty, line)`) or AST with an explicit typed annotation wrapper. An explicit tree ensures emission cannot accidentally access raw un-typed fields and makes call targets explicit.
   *Alternative considered:* A side-table `HashMap<NodeId, Ty>`. *Rejected:* AST nodes do not have persistent IDs across clones and generic instantiations; an explicit typed node structure is clearer, serializable, and cleaner for the LSP.

3. **Call and Method Resolution at Type-Checking Time**:
   All call destinations (functions, methods, generic instances, builtins, closures) are resolved during the checking phase and stored on the call node in the typed tree. Emission simply emits the call to the resolved symbol name.
   *Alternative considered:* Keeping call resolution inside emission. *Rejected:* Defeats the purpose of the typed tree; the checker already knows the concrete signature and receiver type.

4. **Retain `tools/asm-snapshot.py` as the regression gate**:
   Every refactored module must produce identical assembly snapshots for all `examples/*.zila` across Windows, Linux, and unoptimized builds before landing.
   *Refinement (decided while implementing 3.1):* a step that stops emission from compiling something it then throws away (the speculative typing of a `sql` hole, of a literal operand or of a generic argument) no longer uses up label numbers, so every later generated name is numbered differently (`ret_1103` becomes `ret_1054`) while the instructions are the same. Such steps are checked with `python tools/asm-snapshot.py compare --labels`, which renames the numbers of generated names by order of first appearance, per prefix, before comparing. The strict byte comparison stays the gate for every other step.

## Risks / Trade-offs

- [Risk of subtle code generation divergence in edge cases] → `tools/asm-snapshot.py` checks 200+ assembly files, and the full test suite runs at every step.
- [Increased memory during compilation holding both AST and typed tree] → AST is dropped per function after type checking; memory footprint impact is negligible for Zila program sizes.
- [Snapshot rollback complexity in generic expansion] → Generic functions are checked only when instantiated; type-checking of concrete instances happens before emission, obviating textual assembly rollback.
