## 1. The typed program form

- [x] 1.1 Read what `declare`, `layout` and `func_sig` return and write down, in design.md, which state of `Compiler` is checked information and which is emission state (the open question)
- [x] 1.2 Add `TypedProgram` for declarations (records, enums, interfaces, signatures, globals, methods) and make emission read it; assembly of every example is byte-identical to before
- [x] 1.3 Make code generation deterministic (sort the walks over `records` and `enums`) and add `tests/determinism.rs`, which compiles every example twice for three target and flag combinations and compares the text; verify the test fails with the sort removed and passes with it

## Moved to other changes

Generics and call binding, function bodies and the clean-up (old 1.3 to 1.5) moved to `split-checker-from-emission`. The cache of checked packages (old 2.1 to 2.4), its documentation (old 3.1) and the `static-linking` requirement are not started: their design is kept in `design.md` here and in the follow-up section of `split-checker-from-emission`.
