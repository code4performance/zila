## 1. The typed program form

- [x] 1.1 Read what `declare`, `layout` and `func_sig` return and write down, in design.md, which state of `Compiler` is checked information and which is emission state (the open question)
- [x] 1.2 Add `TypedProgram` for declarations (records, enums, interfaces, signatures, globals, methods) and make emission read it; assembly of every example is byte-identical to before
- [ ] 1.3 Move generics instantiation and call binding into the checker output; byte-identical assembly and the whole suite
- [ ] 1.4 Move function bodies (expression and statement types, closures with their captures); byte-identical assembly and the whole suite
- [ ] 1.5 Delete the old path in each area as it is covered, and the `unreachable!()` sites that the typed form makes impossible

## 2. The cache

- [ ] 2.1 Define the cache key and the stored form (versioned, written atomically under `$ZILA_HOME/check/`) and implement reading and writing of the typed form
- [ ] 2.2 Use the cache for packages of a project, with `--no-cache`; a missing or damaged entry is a miss
- [ ] 2.3 Tests: output is byte-identical with and without the cache for every example and for a project with dependencies; a warm build checks nothing again; a changed dependency re-checks it and its users only; other flags and targets are other entries; a deleted or damaged cache gives the same executable
- [ ] 2.4 Measure on `postgres`, `sqlserver`, `ai` and the examples; keep the cache on only where it wins and record the numbers in design.md

## 3. Documentation

- [ ] 3.1 Update `docs/packages.html` (what is cached and where, `--no-cache`) and the CLI listing in `docs/getting-started.html` and `docs/native.html`; verify with `cargo test --test docs`
