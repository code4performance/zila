## ADDED Requirements

### Requirement: Package files use the language's extension
A package SHALL be the `.zila` files directly in its directory, merged into one module, and files ending in `_test.zila` SHALL be left out of builds and included by `--test`; a duplicate declaration across the files SHALL name both files with their `.zila` names. The `use` of a package, the cache and the lock SHALL be unaffected by the extension.

#### Scenario: Duplicate declaration
- **WHEN** `a.zila` and `b.zila` of one package both declare `func f`
- **THEN** the error names `a.zila` and `b.zila`

#### Scenario: Only .zila files
- **WHEN** a dependency's directory contains `lib.zila` and a leftover `lib.br`
- **THEN** the dependency is `lib.zila` and its tree hash still covers every file in the directory
