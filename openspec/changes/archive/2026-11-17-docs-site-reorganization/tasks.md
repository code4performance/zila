## 1. Grouped navigation

- [x] 1.1 Design the grouped nav markup (group headings + links, single `<nav class="site-nav">`, checkbox menu preserved) and add it to `docs/index.html`; verify the page still opens from disk with the mobile menu toggle working without JavaScript
- [x] 1.2 Add the same grouped nav markup to the remaining 13 pages under `docs/`, each with its own `aria-current="page"` on the right link; verify `cargo test --test docs every_page_shares_the_navigation_and_is_linked` (after task 4.1 updates it) passes
- [x] 1.3 Add CSS for the group headings in `docs/styles.css` (non-link, visually distinct, no layout shift at 360px or in print); verify by viewing a page at 360px width and in print preview

## 2. Split the examples page

- [x] 2.1 Create `docs/examples-basics.html`, `docs/examples-comptime.html`, `docs/examples-native.html`, `docs/examples-web-and-ai.html` (flat under `docs/`, not a subdirectory — see design.md), moving each example walkthrough from `docs/examples.html` into the page named in design.md's grouping, each keeping its existing anchor id and labelled code block; verify `cargo test --test docs embedded_examples_match_the_example_files` still passes
- [x] 2.2 Rewrite `docs/examples.html` as a short index page linking to the four topic pages, each topic page also listing its own examples in an "On this page" summary; verify every example shown before the split is reachable from the index in at most one click
- [x] 2.3 Update the four new topic pages' nav to the new grouped markup from task 1.1, nested as the "Examples" entry's sub-pages; verify they appear in the site navigation like every other page
- [x] 2.4 Repoint the six existing cross-page anchors into the old `examples.html` (`index.html#ui`, `native.html#tree-example`, `stdlib.html#json-example`, `stdlib.html#rest-api`, `stdlib.html#web-example`, `syntax.html#unicode-example`, `syntax.html#ffi`) to the matching anchor on its new topic page; verify `cargo test --test docs internal_links_and_anchors_resolve` passes

## 3. Keep the rest of the site working

- [x] 3.1 Confirm `syntax.html` and `stdlib.html` are unchanged apart from their nav markup (per design.md's Non-Goals); verify by diffing each page against its pre-change version outside the `<nav>` block
- [x] 3.2 Run the full `cargo test --test docs` suite and fix anything it reports; verify the suite passes with zero failures (21/22 pass; the one remaining failure, `the_readme_roadmap_matches_the_open_openspec_changes`, is pre-existing and unrelated — it was already failing before this change, caused by the already-archived `verify-linux-debugger-and-editor` change not yet being reflected in README, from before this session started)

## 4. Test suite updates

- [x] 4.1 Update `every_page_shares_the_navigation_and_is_linked` (tests/docs.rs:154) to accept the grouped nav markup and the new file set (four new topic pages replacing the single `examples.html` entry's assumptions); verify the test still fails when a page's nav genuinely drifts (temporarily break one page's nav locally to confirm the test still catches it, then revert) — no code change was needed: `hrefs()` already extracts only `href="..."` occurrences inside the `<nav>` block regardless of the `<p class="nav-group">` headings, and the new topic pages are flat under `docs/` so `pages()` already finds them; verified by temporarily swapping two links' order in `docs/ai.html` and confirming the test failed, then reverting
- [x] 4.2 Update `the_web_library_is_documented` (tests/docs.rs:213) to check `examples-web-and-ai.html` instead of the hardcoded `examples.html`; verify the test passes
- [x] 4.3 Add the "navigation groups drift" and "example unreachable from index" scenarios from the `docs-sync` spec delta as new assertions in `tests/docs.rs` (or confirm an existing assertion already covers them); verify both new scenarios have a passing test and that each fails when the condition it checks is deliberately introduced — "navigation groups drift" is already covered by `every_page_shares_the_navigation_and_is_linked` (confirmed in 4.1); added a new test `every_example_is_reachable_from_the_examples_index` for "example unreachable from index"

## 5. Documentation

- [x] 5.1 No compiler, standard library, CLI, or example source changes — only documentation site structure changes, which is what this change is about; `docs-sync`'s "no user-visible effect" exemption does not apply since the site itself is the affected surface, and tasks 1-4 already cover updating it
