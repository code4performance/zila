## Context

`docs/` is five hand-written static pages (see `add-brasa-docs-site`). Embedded code is copied by hand from `examples/*.br`, so it drifts; nothing checks keywords or flags. The project has no CI and no runtime dependencies, but `cargo test` already runs on every change. See proposal.md for motivation.

## Goals / Non-Goals

**Goals:**
- A docs update that is hard to forget: a process rule plus tests that fail when the site is stale.
- Examples that cannot drift because they are generated from the source files.
- Keep the site static HTML/CSS, dependency-free, openable from disk.

**Non-Goals:**
- Generating prose, or a full API reference, from source.
- A Claude Code hook or CI service; the rule lives in the repository.
- Redesigning the site's look.

## Decisions

**1. Enforcement by tests, not hooks.** Checks live in `tests/docs.rs` and read files with `std` only. Alternatives: a git pre-commit hook (not versioned, bypassable) and a CI workflow (the repo has none). Tests run wherever `cargo test` runs and already gate every change here.

**2. Highlighting markup is stripped before comparing.** The drift check removes tags and decodes the few HTML entities used (`&lt; &gt; &amp; &quot;`), then compares to the file. This keeps the site's hand-tuned classes (`kw`, `str`) while still proving the text is identical. Alternative: generate the whole page; rejected because prose and samples then live in templates that are harder to edit.

**3. A generator for the example blocks.** `brasa-parse --docs` rewrites each `<pre>` block whose preceding `<div class="file-label">` names `examples/NAME.br` from the file, applying keyword and string highlighting from the lexer's token stream, so highlighting follows the real grammar. Alternative: a regex highlighter; rejected because the lexer already knows keywords, strings and comments exactly.

**4. Flags and keywords come from the code, not from a copy.** Keywords are read from `brasa::lexer::KEYWORDS` (already public). Flags are exposed from `main.rs` as a `FLAGS` table used by both argument parsing and the test, so adding a flag without a docs entry fails. Alternative: scrape the `usage()` text; rejected as fragile.

**5. New pages join the shared nav through the same test.** The test derives the page list from `docs/*.html` and requires each page to be linked in every page's nav, which covers the new pages (`native.html`, `concurrency.html`, `os.html`).

**6. The process rule is a project rule in `openspec/config.yaml`.** It tells whoever writes `tasks.md` to include a docs task, or to record why none is needed. This is advisory text for the planning step, so decisions 1-5 are what actually catch omissions.

## Implementation notes

- **Markers are the existing file labels.** The site already shows `<div class="file-label">examples/NAME.br</div>` above each example, so that label is the marker (a label ending in `(excerpt)` is checked as a contiguous part of the file and never rewritten). No new comment syntax was needed. `ffi.br` was an edited excerpt that no longer matched; it now shows the whole file.
- **Shared code lives in the library.** Block finding, text extraction, highlighting and `sync` are in `src/docs.rs`; the option table and argument parser are in `src/cli.rs` (`FLAGS`), so `main.rs`, the generator and `tests/docs.rs` read one source.
- **Snippets are not examples.** The small hand-written snippets of `syntax.html` and `async.html` are language illustrations, not programs from `examples/`; they are verified by compiling them (task 4 steps), not by the drift test.

## Risks / Trade-offs

- [Prose can still go stale] → The checks cover code, flags and keywords only; the rule and review cover prose. Accepted: perfect prose sync is not automatable.
- [Generator disagrees with hand-edited markup] → Only marked blocks are rewritten; unmarked samples that quote an example file are still checked by the same text comparison.
- [Test fails on harmless whitespace] → Comparison trims trailing whitespace and normalizes line endings.
- [Exposing `FLAGS` changes the structure of `main.rs`] → Small, behavior-preserving refactor covered by the existing CLI tests.
