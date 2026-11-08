## ADDED Requirements

### Requirement: Example files are named with .zila
A code block that shows an example file SHALL be labelled `examples/NAME.zila` (or `examples/NAME.zila (excerpt)`), the documentation generator SHALL find the blocks by that label, and the pages SHALL write file names of programs with `.zila`. The check that compares the blocks with the files SHALL keep working.

#### Scenario: A labelled block
- **WHEN** a page has a block labelled `examples/hello.zila`
- **THEN** `zila-parse --docs` and `cargo test --test docs` compare it with that file

#### Scenario: A stale label
- **WHEN** a block is labelled `examples/hello.br`
- **THEN** the docs check fails, saying the file does not exist
