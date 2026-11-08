## Why

The language has a new name (Zila Lang) and still wears the look of the old one. The documentation site is a warm, ember-coloured page with a top bar of twelve links, the only icon is a fire emoji that was part of the old story, and the description of the language, repeated in the README, the site, `Cargo.toml` and the extension, was written for the previous name and only had its name replaced. This change gives the project one identity: a modern, minimal site, an icon, and a description written once and used everywhere.

## What Changes

- **A new site layout**, modern and minimal: a single column of text with generous spacing and a narrow measure, a quiet neutral palette with one accent colour, system fonts (no web font is fetched), a fixed side navigation on wide screens that folds into a menu on narrow ones, light and dark themes that follow the system, code blocks without decoration beyond a thin border, and no JavaScript needed to read a page. The twelve pages and their content stay; the markup that the documentation tests read (the shared `<nav class="site-nav">`, `kw` spans, anchors, example labels) stays.
- **A new icon**: a simple geometric mark for the language as an SVG (also the favicon of every page), a monochrome variant, and a 128 by 128 PNG for the VS Code extension. The fire emoji and the ember colours go.
- **A new description of the language**, written once (a one-line tagline, a one-paragraph summary, a short list of what it is for) in `docs/brand.md`-style source text and used by the site front page, the README, `Cargo.toml`, the extension's `package.json` and README, and the page `<meta>` descriptions. It says what the language is and does, with claims the repository backs up.
- **The Markdown files** (`README.md`, `CONTRIBUTING.md`, `GOVERNANCE.md`, `CODE_OF_CONDUCT.md`, `zila-ffi.md`, `editors/vscode/README.md`, `packages/README.md`) get the same voice: the new description and a consistent header with the icon.
- A test that fails when a page lacks the icon link, the description or the shared navigation.

## Capabilities

### New Capabilities
- `brand-identity`: the icon and the single description of the language, and where it is used.

### Modified Capabilities
- `docs-site`: the layout, theme, navigation behaviour and accessibility of the pages.
- `vscode-extension`: the extension has an icon and a description that match the brand.

## Impact

- `docs/styles.css` (rewritten), the header and footer markup of the 12 pages in `docs/`, new `docs/assets/` (icon files), `README.md` and the other Markdown files, `Cargo.toml` and `editors/vscode/package.json` (descriptions, icon), `editors/vscode/` (the PNG), new tests in `tests/docs.rs`.
- No change to the language, the compiler or the page content beyond the front page text. The docs checks (examples match files, flags, keywords, anchors, shared navigation) must keep passing.
- Non-goals: new documentation content, a site generator or any build step for the site (it stays hand-written static HTML), a custom domain, translations, a logo for the operating-system image, and renaming anything again.
- Assumption to confirm: the meaning of the name "zila" is not given anywhere, so the description and the icon do not claim one; the icon is a letter mark, not a picture of something.
