## ADDED Requirements

### Requirement: Incremental builds give identical output
When the cache of checked packages is used, a build SHALL produce an executable byte-identical to the build with the cache disabled (`--no-cache`), and SHALL check a package again whenever its source, its dependencies' sources, the compiler version, the target or the flags change. A package SHALL NOT be checked again when none of them changed.

#### Scenario: Warm cache
- **WHEN** a project is built twice without changes, the second time with a warm cache
- **THEN** both executables are identical and the second build checks no package again

#### Scenario: Changed dependency
- **WHEN** one source file of a dependency changes
- **THEN** that package and the packages that use it are checked again and the others are taken from the cache

#### Scenario: The cache is only an optimisation
- **WHEN** the cache directory is deleted, or a cached entry is damaged
- **THEN** the build succeeds with the same executable, checking the packages again

#### Scenario: Other flags are other entries
- **WHEN** the same package is built with `--no-opt`, and for `--target linux`
- **THEN** each build uses its own cache entry and none returns the result of another
