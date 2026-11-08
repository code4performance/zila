## Context

The site is 12 hand-written static pages in `docs/` sharing `docs/styles.css` (about 4 KB, warm "ember" colours, a sticky top bar with twelve links). `tests/docs.rs` reads the HTML: the shared `<nav class="site-nav">`, `kw` spans against the language's keywords, anchors and links, example blocks against `examples/*.br`, documented flags. The Markdown files were written for the old name and had only the name replaced. The only icon is a fire emoji in the README title (removed in the rename). The VS Code extension has no icon.

## Goals / Non-Goals

**Goals:**
- A modern, minimal site that is fast and readable on a phone, in the dark, with a keyboard and in print.
- An icon that works at 16 px (favicon) and 128 px (extension) and in one colour.
- A description written once and checked everywhere it is repeated.

**Non-Goals:**
- A site generator or a build step, new documentation content, JavaScript, web fonts, translations, a domain.

## Decisions

1. **Keep the pages hand-written and the markup contract.** Only the stylesheet, the header and footer markup and the front page text change. The `<nav class="site-nav">` block, `kw` spans, anchors and example labels stay as they are, so `tests/docs.rs` keeps guarding the content. A one-off script (`tools/apply-layout.py`, not part of any build) rewrites the header and footer of the 12 pages identically, and the shared-navigation test then keeps them identical.
2. **Layout.** One reading column (`max-width: 68ch`) beside a fixed side navigation on screens of 800 px and wider (CSS grid, two columns); below that the navigation is behind a "Menu" button made of a checkbox and its label (a `<details>` element cannot be forced open on wide screens in every browser, and a second copy of the navigation would break the one-navigation-per-page rule), so no script is needed. A "Skip to content" link comes first. Headings use a modest scale (1.25 ratio), generous vertical rhythm, and only one weight change. Alternative: a top bar with a dropdown; rejected, because twelve links do not fit a top bar on a phone and a side list shows where you are.
3. **Palette as tokens.** Neutral greys, one accent (a calm blue-violet, `#5b5bd6` light, `#8e8ef0` dark, both above 4.5 contrast on their backgrounds), defined once as CSS custom properties with a `prefers-color-scheme: dark` override. Code blocks use the background of the page with a 1 px border instead of the old inverted dark box; keywords, strings and comments get three muted colours with enough contrast. The ember colours and the `ember-dot` class are removed from the stylesheet and the markup.
4. **System fonts only.** `system-ui` for text and `ui-monospace, "Cascadia Code", Consolas, monospace` for code. Nothing is fetched; the site works from disk.
5. **The icon is a letter mark, not a picture.** Nothing in the repository says what "zila" means, so the icon does not claim a meaning. It is a rounded square holding a geometric Z made of two horizontal bars joined by a diagonal, drawn with filled paths (no text, no strokes that scale badly), on the accent colour with the Z in white; the mono variant is the Z alone in `currentColor`. It stays legible at 16 px because the bars are thick and the shape has no fine detail. The SVG is the source; the 128 px PNG is made from the same geometry by `tools/make-icon.py` (standard library only: fills the polygons at 4x supersampling and writes the PNG with `zlib`), and a test checks that the PNG is 128 by 128.
6. **The description is data.** `docs/assets/description.txt` has three fields, `tagline:`, `summary:` and `for:` (one item per line). A test reads it and checks that the front page, the README, `Cargo.toml`, the extension's `package.json` and README, and the front page `<meta>` contain the tagline or summary verbatim. Proposed text (the owner may change it before apply):

   - tagline: *A small, readable language that compiles straight to machine code.*
   - summary: *Zila Lang is a compiled, statically typed language with readable syntax, where blocks close with the word end. Its compiler, written in Rust with no dependencies, produces x86-64 programs for Windows and Linux itself: no virtual machine, no garbage collector, no runtime to install. Errors are values, memory is reference-counted, and the same language writes applications, C bindings and a kernel. (Plain text, without markup, so that it can be used as it is in HTML and Markdown.)*
   - for: command-line tools and servers; native desktop windows; bindings to C libraries; operating-system code.

   Every claim is one the repository demonstrates (the PE and ELF writers, the optimizer-free runtime, `T or error`, reference counting, the `os/` kernel).
7. **The Markdown files get one voice.** Each starts with the icon and the tagline, then a short paragraph; the long README keeps its sections but loses the old story and the emoji; `CONTRIBUTING.md`, `GOVERNANCE.md` and `CODE_OF_CONDUCT.md` keep their content and only fix wording that still read as the old project.
8. **Accessibility is checked, not assumed.** Contrast pairs are listed as tokens in the stylesheet header comment and a test computes their ratios from the custom properties; the skip link, `aria-current`, `lang` and focus styles are checked by reading the HTML and CSS.

## Risks / Trade-offs

- A minimal design can look unfinished; mitigation: spend the care on typography and spacing, and review screenshots at 360, 800 and 1280 px, light and dark, before calling it done.
- Rewriting the header of 12 pages can drift; mitigation: one script writes them and the existing shared-navigation test fails on any difference.
- The PNG is generated by a small rasteriser instead of a design tool; mitigation: the shape is simple, supersampling hides the jaggies, and the SVG stays the source of truth.
- A description checked verbatim is rigid; mitigation: it is one file, and changing it is one edit plus the files the test names.

## Open Questions

- Are the proposed tagline and summary right, and is a letter-mark Z the icon wanted? Answer before apply; the rest does not depend on it.
