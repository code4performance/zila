## Context

See proposal.md - Why. Relevant current state, confirmed by reading `tests/docs.rs` and the site itself:

- `every_page_shares_the_navigation_and_is_linked` (tests/docs.rs:154) asserts byte-for-byte identical `href` lists, in the same order, across every page in `docs/`, **and** that every `.html` file present in `docs/` is linked from that nav. Splitting `examples.html` into several files means the nav must grow to include every new file, or this test fails — grouping headings alone don't violate it (headings add no `href`), but adding/removing pages does.
- Six existing cross-page links point at anchors inside `examples.html` specifically: `index.html` (`#ui`), `native.html` (`#tree-example`), `stdlib.html` (`#json-example`, `#rest-api`, `#web-example`), `syntax.html` (`#unicode-example`, `#ffi`). Every one of these must be repointed to wherever that example lands after the split.
- `the_web_library_is_documented` (tests/docs.rs:213) hardcodes `page("examples.html").html.contains("stdlib.html#web")` — this must be updated to check whichever topic page ends up hosting the web example.
- `embedded_examples_match_the_example_files` (tests/docs.rs:73-101) walks every page's `blocks()` and compares each labelled code block to its `examples/*.zila` source; it is page-agnostic, so splitting pages does not break it as long as each block keeps its label.
- `syntax.html` and `stdlib.html` already have an in-page "On this page" TOC (`<nav class="toc">`) — `examples.html` has the same pattern today; the split keeps each new topic page in that same pattern rather than introducing a new one.

## Goals / Non-Goals

**Goals:**
- A grouped top-level nav and a split examples section that a first-time visitor can scan in a few seconds, without changing any of the 16 example walkthroughs' content or any prose on the non-split pages.
- Every existing deep link (the six `examples.html#...` anchors above, plus anything a search engine indexed under the old nav structure) is deliberately repointed, not silently broken.

**Non-Goals:**
- Not touching `syntax.html` or `stdlib.html`'s own structure — they already have working in-page TOCs; this change does not re-paginate them.
- Not adding content coverage for the ~22 example files under `examples/` that `examples.html` does not currently show (e.g. `allocator.zila`, `threads_sum.zila`) — reorganizing what exists, not expanding what's documented.
- Not changing `docs/styles.css`'s color system, typography, or theme handling beyond what grouped-nav headings need.

## Decisions

- **Grouping is headings inside one `<nav>`, not multiple `<nav>` elements.** `docs-site`'s existing requirement already mandates "a single `<nav>` element" for the mobile checkbox-menu trick to keep working without JavaScript; grouping must stay inside that one element as non-link group headings (e.g. `<p class="nav-group">Language</p>` followed by the group's links). Alternative considered: separate `<nav>` per group; rejected because it breaks the existing single-`<nav>`-element mobile menu mechanism the spec already depends on.
- **Examples split by audience, not by file count.** Four topic pages, flat under `docs/` (not a subdirectory — see below): `examples-basics.html` (Hello world, Type system tour, defer, Maps, Text and lists, Text and characters, Generics, Floats), `examples-comptime.html` (comptime: code that writes code), `examples-native.html` (Native windows, Async, FFI & low-level, tree), `examples-web-and-ai.html` (REST API, json, web). The short index page stays at `docs/examples.html`. Alternative considered: one page per example (16 pages); rejected as more files to add to the nav for no navigational benefit — a visitor wants "show me the native examples," not sixteen separate menu entries.
- **Topic pages stay flat under `docs/` (hyphenated names), not in a `docs/examples/` subdirectory.** Two reasons, both found by reading the existing test suite and spec before implementing: (1) `tests/docs.rs`'s `pages()` helper reads only the direct contents of `docs/` (`fs::read_dir("docs")`, not recursive) — a subdirectory's pages would be invisible to every test unless that helper were also made recursive; (2) `docs-site`'s unmodified "Self-contained static site" and "Navigation" requirements need the nav markup to be byte-identical on every page and the site to work opened directly from `file://` with no server — a page one directory deeper needs different relative paths (`../index.html` vs `index.html`) to reach the same targets, which a subdirectory would make impossible without also changing those requirements. Flat, hyphenated filenames avoid both problems with no requirement changes needed beyond what's already in this change's spec delta.
- **The index page keeps the URL `examples.html`.** Every existing link that points at `examples.html` without an anchor (the nav, and any external bookmark) keeps working; only the six anchored links need repointing to `examples-<topic>.html#<anchor>`.
- **Nav gains exactly four new entries** (the four topic pages), nested under the existing "Examples" link's group rather than at the top level, keeping the top-level group count the same five groups from the proposal.
- **Update `tests/docs.rs` in the same change** (`every_page_shares_the_navigation_and_is_linked` to tolerate a nested/grouped nav and the new file set; `the_web_library_is_documented`'s hardcoded `examples.html` check to point at `examples/web-and-ai.html`) rather than leaving the spec change unverified by its own test suite.

## Risks / Trade-offs

- [A repointed anchor is missed, leaving a dead link] → `internal_links_and_anchors_resolve` (tests/docs.rs:171) already checks every internal link/anchor pair across the whole site; running it after the split catches any miss mechanically, not by manual review.
- [Grouped nav headings make the flat-list keyboard/skip-link behavior (Requirement: Navigation, "Keyboard" scenario) harder to keep correct] → Keep group headings as non-interactive text (no `tabindex`, no link), so Tab order is unaffected; verify with the existing keyboard scenario.
- [Splitting examples.html changes the file four new pages need their own entries in `docs-sync`'s "every example reachable from the index" check] → Already covered by the new scenario added to `docs-sync`'s Site integrity requirement in this change's spec delta.
- [External search engines or bookmarks to `examples.html#anchor` (outside this repo) break] → Accepted; no redirect mechanism exists for a static file-based site with no server, and the six in-repo links are what's actually verified by tests.
