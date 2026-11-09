## 1. Oracle

- [x] 1.1 Add `tools/asm-snapshot.py` that writes the assembly of every example for the default build, `--target linux` and `--no-opt`, and compares two snapshots; verify it reports no difference between two runs of the same compiler and one when a line of the compiler's output is changed

## 2. Generics results

- [x] 2.1 Move the instantiation results (`inst_args`, `inst_funcs`, `pending`, `inst_count`, `map_thunks`) into `TypedProgram`; byte-identical assembly and the whole suite

## 3. Typing without emitting

- [ ] 3.1 A checker `type_of` that types an expression without emitting, one expression form at a time, starting with what calls need (literals, identifiers, field access, calls); while a form is migrated, every `expr` call also checks (in tests) that its result type equals `type_of`'s; byte-identical assembly after each form
- [ ] 3.2 Statements, closures with their captures and the rest of what `Snapshot` rolls back; byte-identical assembly and the whole suite

## 4. Call binding

- [ ] 4.1 Record the target each call resolves to in the typed form (calls by name, then calls by the type of the receiver using `type_of`) and make emission read it; byte-identical assembly and the whole suite

## 5. Clean-up

- [ ] 5.1 Delete the old path in each area as it is covered, and the `unreachable!()` sites the typed form makes impossible; verify with a search and the whole suite
