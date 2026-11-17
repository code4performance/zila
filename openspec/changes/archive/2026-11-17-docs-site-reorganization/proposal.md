## Why

The documentation site (`docs/`) has grown to 14 HTML pages sharing one flat, 13-link navigation list with no grouping, and `examples.html` has grown to 977 lines covering 16 unrelated example programs on a single page. A newcomer arriving from the open-source launch (see `open-source-public-launch`) has no way to tell, from the nav alone, which pages are "the language," which are "the standard library," and which are "advanced/native" — everything sits at the same level. This change reorganizes the site's information architecture so it is easy to understand and navigate, without changing what the site says about the language.

## What Changes

- Group the 13 site navigation links into labeled sections (e.g. **Language** — Getting Started, Syntax, Examples; **Standard Library** — Library, Databases, AI; **Runtime & Native** — Native, Tasks, Async; **Tooling** — Packages, Editors; **Zila Lang OS**), rendered as a single `<nav class="site-nav">` with grouped headings inside it. Link set, targets, and the overall page content are unchanged — only the navigation's internal structure and the examples page's file layout change.
- Split `docs/examples.html` (977 lines, 16 examples) into a short index page plus topic pages (e.g. basics, comptime, native/concurrency, web/AI), named flat under `docs/` (e.g. `examples-basics.html`), each with its own "On this page" TOC, mirroring the pattern `syntax.html` and `stdlib.html` already use for their own internal TOCs. Every example currently shown stays shown; only the page that hosts it changes.
- Keep `stdlib.html` and `syntax.html` as single pages (each already has an in-page TOC) — reviewed but not split, since their internal navigation is already addressed.
- Update the `docs-site` spec's Navigation requirement (currently "the same links in the same order, unchanged markup") to describe grouped navigation instead of a flat list, and the "Annotated examples" requirement to describe examples organized across topic pages instead of implicitly one page.
- Update the `docs-sync` spec's Site integrity requirement and its automated check so "every page shares the same navigation" still holds for the new, grouped nav, and "every internal link resolves" is re-verified against the new examples topic pages.

**BREAKING**: none for language users — no compiler, stdlib, or CLI behavior changes. Internal: any bookmark or external link to a specific example's anchor on the old single `examples.html` page will need to be updated once examples move to topic pages (listed as a migration step in tasks.md).

## Capabilities

### New Capabilities
(none)

### Modified Capabilities
- `docs-site`: the Navigation requirement changes from an unchanged flat list to a grouped navigation structure; the Annotated examples requirement changes from (implicitly) one examples page to examples organized across topic pages under a shared index.
- `docs-sync`: the Site integrity requirement's check needs to validate the new grouped navigation markup and the new examples topic-page links, instead of the old flat list and single examples page.

## Impact

- **Affected files**: all 14 pages under `docs/` (shared nav markup changes), `docs/examples.html` (split into an index + topic pages), `docs/styles.css` (styling for grouped nav), and whatever test(s) currently assert nav/link consistency (likely under `tests/docs.rs` — to confirm in design.md).
- **No changes** to the compiler, standard library, CLI, or any example program's source under `examples/`.
- **Coordinates with** `open-source-public-launch`, whose `launch-marketing` capability adds a contributor-facing section to `docs/index.html` — that section's placement should respect the new grouped nav structure, not fight it.
