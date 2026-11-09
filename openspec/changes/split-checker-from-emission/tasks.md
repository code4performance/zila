## 1. Oracle

- [ ] 1.1 Add `tools/asm-snapshot.sh` (or an equivalent) that writes the assembly of every example for the default build, `--target linux` and `--no-opt`, and compares two snapshots; verify it reports no difference between two runs of the same compiler and one when a line of the compiler's output is changed

## 2. Generics and call binding

- [ ] 2.1 Move the instantiation results (`inst_args`, `inst_funcs`, `pending`, `inst_count`, `map_thunks`) into `TypedProgram`; byte-identical assembly and the whole suite
- [ ] 2.2 Record the target each call resolves to in the typed form and make emission read it; byte-identical assembly and the whole suite

## 3. Function bodies

- [ ] 3.1 A checker that types expressions without emitting, one expression form at a time, the old method delegating to it; byte-identical assembly after each form
- [ ] 3.2 Statements, closures with their captures and the rest of what `Snapshot` rolls back; byte-identical assembly and the whole suite

## 4. Clean-up

- [ ] 4.1 Delete the old path in each area as it is covered, and the `unreachable!()` sites the typed form makes impossible; verify with a search and the whole suite
