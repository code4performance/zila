## Purpose

Show the consumer of a package what it can do to the machine and to memory safety, so trusting a dependency is an informed choice and not only a hash match.

## ADDED Requirements

### Requirement: Effects of a package
For every package in a build, the toolchain SHALL determine its effects: whether its code (files not ending in `_test.br`) contains an `unsafe` block, an `extern` declaration or an `asm` block, and for `extern` which libraries it names. The effects of a package SHALL be those of its own code only, not those of its dependencies, which are reported separately. In a build report (`--explain-link`) they SHALL be those of the code the program reaches (generic code always counts); when packages are downloaded, where no program exists, and for the policy, they SHALL be those of the whole package, tests left out.

#### Scenario: A package with FFI
- **WHEN** a dependency declares `extern func` from `user32` and has no `unsafe` block elsewhere
- **THEN** its effects are `extern (user32)`

#### Scenario: A pure package
- **WHEN** a package has none of the three constructs
- **THEN** its effects are reported as `none`

### Requirement: Effects are shown when a dependency is fetched and in the link report
`--fetch`, `--add` and `--update` SHALL print the effects of each package they download or whose version changed, and `--explain-link` SHALL print them next to the size of each package. An increase in the effects of a locked package after `--update` SHALL be marked as a change.

#### Scenario: An update gains an effect
- **WHEN** `--update` moves a package from effects `none` to `unsafe`
- **THEN** the output names the package, the old and the new effects, and marks the line as changed

### Requirement: Policy file
When `zila.toml` contains `[policy]` with `forbid = "unsafe"`, `"extern"` or `"asm"` (a single string; the manifest subset has no arrays), a build SHALL fail if any dependency, direct or transitive, has that effect, naming the package and the construct. The project's own code is not checked.

#### Scenario: Forbidden effect
- **WHEN** `[policy]` has `forbid = "asm"` and a dependency has an `asm` block
- **THEN** the build fails naming the dependency and the file
