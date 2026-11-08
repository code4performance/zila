## Why

The documentation site (`docs/`) was written once and has already fallen behind the language: it knows nothing about the native compiler (`--build`), reference counting, `spawn`/`channel`, `or error`/`try` beyond one edit, the `ui` and `task` libraries, or Zila Lang OS, and its code samples were edited by hand, so nothing stops them drifting from `examples/*.br` and the grammar. Documentation that is only updated when someone remembers is wrong most of the time. The request is that every change to the language keeps the site current.

## What Changes

- Add a **docs-sync rule** to the project's OpenSpec configuration so that every future change that touches the language, compiler, standard library, CLI or examples must include tasks that update the affected `docs/` pages, and cannot be considered done without them.
- Add **automated drift checks** that run with `cargo test`:
  - every program embedded in `docs/examples.html` matches the corresponding file in `examples/` (modulo the HTML highlighting markup);
  - every command-line flag the CLI accepts is documented in `docs/getting-started.html`, and every flag documented there exists;
  - every reserved word in `src/lexer.rs` is mentioned in `docs/syntax.html`, and the site mentions no keyword the lexer no longer has (for example `union`);
  - the docs build their navigation consistently and have no broken internal links.
- Make the embedded examples **generated, not hand-written**: a small `cargo run --bin` tool (or a `zila-parse --docs` mode) renders `examples/*.br` into the HTML blocks, so updating an example updates the site.
- **Bring the site up to date now** with everything already built: native compilation and its supported subset, memory model (reference counting), error handling with `or error`/`try`, enums with data and exhaustive `match`, `spawn`/`channel`, the `ui` library, and a Zila Lang OS page (boot sector, kernel, how to run it in QEMU).
- No change to the language, compiler or runtime behavior.

## Capabilities

### New Capabilities
- `docs-sync`: guarantees that the documentation site reflects the current language: a process rule for changes, automated checks that fail when docs and source disagree, and generated example blocks.

### Modified Capabilities
<!-- None: the earlier add-zila-docs-site change has not been archived, so there is no main docs-site spec yet. The new pages below are covered by the docs-sync spec. -->

## Impact

- **Affected code**: `openspec/config.yaml` (rule), a new test file `tests/docs.rs`, a small docs generator (new `src/bin/` file or a CLI mode), and the pages under `docs/` (`index`, `syntax`, `examples`, `getting-started`, plus new `native.html`, `concurrency.html` and `os.html`).
- **Dependencies**: none; the checks use only `std` and the existing crate, consistent with the project's no-dependencies rule.
- **Assumptions** (no clarification was needed, but these shape the design):
  - "Whenever there are changes" is enforced by process plus tests, not by a Claude Code hook or CI service, because the repository has neither and the tests already run on every `cargo test`.
  - The site stays plain static HTML/CSS that opens from disk without a server.
  - Prose is still written by hand; only code samples and the keyword/flag lists are checked or generated.
