## MODIFIED Requirements

### Requirement: Resolving `use`
For `use a.b.c`, the first segment SHALL be looked up among the core standard library, the project's declared dependencies, the local package directories next to `brasa.toml` and the standard libraries outside the core; a name SHALL exist in only one of the first three, and only the core standard names SHALL be reserved. A dependency or local package whose name equals a standard library outside the core SHALL shadow it, and the build SHALL print a warning naming both. Later segments SHALL select a subdirectory of the package, and the name bound in code SHALL be the last segment. A package SHALL be the `.br` files directly in its directory, merged into one module; a duplicate declaration across them SHALL be an error naming both files; files ending in `_test.br` SHALL be left out of builds. A dependency's own `use` SHALL be resolved with its own manifest. A name found nowhere SHALL be an error that suggests `--add`.

#### Scenario: Dependency and sub-package
- **WHEN** `use json.stream` is written and `json` is a locked dependency with a `stream/` directory
- **THEN** `stream.<name>` refers to the declarations of `stream/*.br`

#### Scenario: Reserved name
- **WHEN** `--add` would give a dependency the import name `text`, a core standard library
- **THEN** the tool refuses, saying the name belongs to the core standard library, and changes nothing

#### Scenario: Shadowing a non-core library
- **WHEN** a dependency is named `json` and `json` is also a standard library outside the core
- **THEN** `use json` refers to the dependency and the build prints a warning that it shadows the standard one

#### Scenario: Unknown name
- **WHEN** a program has `use nothing` and no manifest, local directory or standard library of that name
- **THEN** compilation fails with `unknown package 'nothing'` and the `--add` hint

#### Scenario: Different versions below
- **WHEN** two dependencies each `use json` and their manifests select different majors
- **THEN** each is compiled against its own, with no symbol clash
