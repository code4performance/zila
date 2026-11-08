# brand-identity Specification

## Purpose
One identity for the language wherever it is shown: the same icon and the same description.

## Requirements

### Requirement: The icon
The repository SHALL contain the language's icon as an SVG mark (`docs/assets/zila.svg`), a single-colour variant (`docs/assets/zila-mono.svg`) and a PNG of 128 by 128 pixels for the VS Code extension. The SVG SHALL work on light and dark backgrounds, SHALL contain no text, no embedded raster image and no script, and SHALL be under 2 KB. Every page of the site SHALL link it as its favicon, and the README and the extension SHALL show it.

#### Scenario: Every page has the icon
- **WHEN** any page of `docs/` is read
- **THEN** its `<head>` has a `rel="icon"` link to `assets/zila.svg` and the file exists

#### Scenario: A plain mark
- **WHEN** `docs/assets/zila.svg` is read
- **THEN** it has no `<script>`, no `<image>`, no `<text>` and is smaller than 2 KB

### Requirement: One description
The description of the language SHALL be written once, in `docs/assets/description.txt`, as a tagline of at most 90 characters, a summary of at most 500 characters and a list of what the language is for. The front page of the site, the README, `Cargo.toml`, the extension's `package.json` and README, and the `<meta name="description">` of the front page SHALL use that text (the tagline or the summary as fits), and a test SHALL fail when one of them differs. The text SHALL say only what the repository can show: a compiled, statically typed language that writes x86-64 machine code itself, with no virtual machine, collector or runtime to install, readable `end`-delimited syntax, errors as values, reference-counted memory, and one language from applications to a kernel.

#### Scenario: Same words everywhere
- **WHEN** the tagline in `docs/assets/description.txt` is changed and nothing else
- **THEN** the description test fails and names the files that no longer match
