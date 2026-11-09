## 1. Oracle

- [x] 1.1 Add `tools/asm-snapshot.py` that writes the assembly of every example for the default build, `--target linux` and `--no-opt`, and compares two snapshots; verify it reports no difference between two runs of the same compiler and one when a line of the compiler's output is changed

## 2. Generics results

- [x] 2.1 Move the instantiation results (`inst_args`, `inst_funcs`, `pending`, `inst_count`, `map_thunks`) into `TypedProgram`; byte-identical assembly and the whole suite

## 3. Typing without emitting

- [x] 3.1 A checker `type_of` that types an expression without emitting, one expression form at a time, starting with what calls need (literals, identifiers, field access, calls); while a form is migrated, every `expr` call also checks (in tests) that its result type equals `type_of`'s; byte-identical assembly after each form

## Not done, left for a later change

The rest of the plan needs emission to walk a typed tree instead of the syntax tree (see "Step 3.1 closed" in design.md): the forms `type_of` does not cover yet, statements, closures with their captures and what `Snapshot` rolls back; recording the target of each call as the input of emission; deleting the old typing and the `unreachable!()` sites it makes impossible. They are not started and no longer belong to this change. The cache of checked packages stays behind them (follow-up in design.md).
