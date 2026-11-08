## Why

Zila Lang currently has no single place for newcomers to learn what it is, why it exists, or how to read its syntax — the only references are `README.md` (parser-implementation notes), `zila.ebnf` (raw grammar), `zila-ffi.md`, and the `.br` files under `examples/`. Someone evaluating or learning the language has to piece it together from a grammar file and scattered example programs. A static documentation site gives Zila Lang a readable front door: purpose, syntax tour, and real examples in one place.

## What Changes

- Add a static, browsable documentation website (plain HTML/CSS, no build step required) covering:
  - **Overview**: what Zila Lang is, its design goals, and who it's for.
  - **Syntax guide**: core language constructs (packages/`use`, `record`/`union`/`enum`/`interface` types, functions, generics, `match`, error handling with `!`/`?`, `defer`, `ref` params, string interpolation, attributes, `asm`/FFI) derived from `zila.ebnf` and the existing `.br` examples.
  - **Examples**: annotated walkthroughs built from `examples/*.br` (hello world, REST API, type system tour, FFI/low-level).
  - **Getting started**: how to build the parser and run `.br` files with `zila-parse` (`--ast`, `--tokens`, `--run`).
- Site content is generated from the existing grammar/example source files where practical, so it does not drift from the actual parser behavior.
- No changes to the Rust parser, lexer, or interpreter themselves.

## Capabilities

### New Capabilities
- `docs-site`: a static documentation website explaining Zila Lang's purpose, syntax, and usage through annotated examples.

### Modified Capabilities
_None — this change only adds documentation content and does not alter parser/interpreter behavior._

## Impact

- **Affected areas**: new `docs/` (or `site/`) directory of static HTML/CSS (and optionally a tiny build script) added to the repo; no changes to `src/`, `Cargo.toml`, or parser behavior.
- **Dependencies**: none required; plain HTML/CSS keeps the site dependency-free and consistent with the project's "no dependencies" philosophy.
- **Maintenance**: syntax/example content should be kept in sync with `zila.ebnf` and `examples/*.br` as the language evolves.
