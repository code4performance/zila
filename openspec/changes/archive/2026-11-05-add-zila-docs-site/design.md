## Context

Brasa is a parser/interpreter project (Rust, std-only, no dependencies — see `README.md`) with a formal grammar in `brasa.ebnf`, FFI notes in `brasa-ffi.md`, and sample programs in `examples/*.br`. There is no project-facing documentation site today; see `proposal.md` - Why. The site must coexist with the Rust crate without affecting `cargo build`/`cargo test`.

## Goals / Non-Goals

**Goals:**
- Ship a static, dependency-free HTML/CSS site that can be opened directly from disk (`file://`) or hosted as-is (e.g., GitHub Pages) with zero build step.
- Keep documented syntax traceable back to `brasa.ebnf` and `examples/*.br` so content doesn't silently diverge from the parser.
- Keep the site easy to extend as new language features land.

**Non-Goals:**
- No in-browser Brasa playground/interpreter (the Rust interpreter is not compiled to WASM as part of this change).
- No search, versioning, or multi-language (i18n) support.
- No static-site-generator toolchain (Jekyll, Hugo, Eleventy, etc.) or JS build pipeline.
- No automated content-sync tooling that regenerates pages from `brasa.ebnf`/`examples/*.br` — content is hand-authored and manually cross-checked (see Risks).

## Decisions

**Plain hand-authored HTML/CSS, no generator or framework.**
Brasa's own README emphasizes "no dependencies (std only)"; mirroring that philosophy for the docs site avoids introducing a Node/Ruby/Python toolchain just to render a handful of pages. Alternative considered: a static site generator (Jekyll/Eleventy) — rejected because it adds a build step and dependency surface disproportionate to ~5 pages, and complicates "open index.html and it just works."

**Location: top-level `docs/` directory.**
Placing the site at `docs/` (not `site/`) lets it be served directly by GitHub Pages' "Deploy from a branch → /docs" option with no extra configuration if the repo is later published that way, while remaining a plain static folder otherwise. Alternative considered: `site/` — rejected only because it requires an extra step (Pages custom workflow) to host later; no functional difference for local/offline viewing.

**Page structure: one HTML page per major section, shared stylesheet.**
`docs/index.html` (overview + nav), `docs/syntax.html`, `docs/examples.html`, `docs/getting-started.html`, all linking a single `docs/styles.css`. Keeps each page small and independently linkable, matches the capability's four requirements (overview, syntax reference, examples, getting started) one-to-one, and avoids a single huge page that's hard to navigate without JS-based routing.

**Code samples via `<pre><code>` with hand-applied `<span>` classes for keyword highlighting, no JS highlighter library.**
Keeps the site dependency-free and fully functional offline/without network access (a CDN-hosted highlighter like highlight.js would silently lose styling offline). Trade-off: less polished highlighting than a JS library; acceptable given the content volume (a handful of code samples, not a code-heavy reference manual).

**Examples sourced by copy-and-annotate, not auto-extraction.**
Each example page embeds the relevant `.br` source (copied from `examples/`) next to hand-written prose. Alternative considered: a small script that extracts `examples/*.br` into the HTML at build time — rejected as a non-goal (no build step), but noted as a future option if drift becomes a problem.

## Risks / Trade-offs

- **Content drift**: hand-copied syntax examples and grammar descriptions can fall out of sync as `brasa.ebnf`/`examples/*.br` evolve. → Mitigation: tasks.md includes an explicit cross-check step against `brasa.ebnf` and `examples/*.br` before marking the change complete; future changes touching the grammar should include a docs-site update in their own task list.
- **Manual syntax highlighting is labor-intensive and inconsistent**: hand-applied `<span>` classes are easy to get slightly wrong or skip. → Mitigation: keep highlighting minimal (keywords/strings/comments only) and prioritize correctness of the code sample text over highlighting completeness.
- **No automated tests for static content**: nothing fails CI if a page goes stale or a link breaks. → Mitigation: tasks.md includes a manual link-check pass; broken internal links are directly observable by clicking through the site before calling the change done.

## Migration Plan

Purely additive — a new `docs/` directory with no changes to `src/`, `Cargo.toml`, or existing files. No rollback concerns beyond removing the directory if ever needed.
