## 1. The earlier name (done, then retired)

- [x] 1.1 The notes about the earlier name that the rename tool had damaged were first rewritten, then removed together with the tool, its test and the `rename-language` change when the name was taken out of the whole repository (owner's request); verify that no tracked file, file name, archive entry or Word document has the old name

## 2. Brand: icon and description

- [x] 2.1 Draw `docs/assets/zila.svg` and `docs/assets/zila-mono.svg` (letter mark, filled paths, no text or script, under 2 KB); verify legibility at 16, 32 and 128 px in light and dark
- [x] 2.2 `tools/make-icon.py` (standard library only) producing `editors/vscode/icon.png` at 128 by 128 from the same geometry; verify the size and that the file is a valid PNG
- [x] 2.3 `docs/assets/description.txt` with the tagline, summary and list, once the owner has confirmed or changed the proposed text (open question in design.md)
- [x] 2.4 Tests in `tests/docs.rs`: every page links the icon and the file exists; the SVG has no script, image or text and is small; the description appears verbatim in the front page, README, `Cargo.toml`, `package.json` and the extension README; the PNG is 128 by 128

## 3. The site

- [x] 3.1 Rewrite `docs/styles.css`: tokens, one column of 68ch, side navigation from 800 px, `<details>` menu below, light and dark, print rules, focus styles, no ember colours; verify at 360, 800 and 1280 px
- [x] 3.2 `tools/apply-layout.py` (one-off) writing the same header (skip link, brand with the icon, menu and `<nav class="site-nav">` unchanged), footer and favicon link into the 12 pages; verify the shared-navigation test passes
- [x] 3.3 Rewrite the front page text with the description and remove the ember dot and old classes from every page; verify `cargo test --test docs`
- [x] 3.4 Check the pages: no external requests, no horizontal scroll at 360 px except code blocks, keyboard order starts at the skip link, contrast pairs computed from the tokens are at least 4.5 in both themes (a test for the ratios); verify by test and by looking at each page

## 4. The Markdown files

- [x] 4.1 `README.md`: icon, tagline and summary first, the list of what it is for, the old story and the title emoji gone; the sections below are unchanged and keep their status marks; `docs` links still resolve (`cargo test --test docs` passes)
- [x] 4.2 `CONTRIBUTING.md`, `GOVERNANCE.md`, `CODE_OF_CONDUCT.md`, `zila-ffi.md`, `packages/README.md`: same voice, icon and tagline where it fits, wording that still read as the old project fixed
- [x] 4.3 `Cargo.toml` and `editors/vscode/package.json` descriptions, the `icon` field, the extension README header; verify the extension tests and that `.vscodeignore` keeps the icon

## 5. Verification

- [x] 5.1 No documentation page lists the files that changed (the repository listing is in README.md, which now names `packages/` and `tools/`); the content pages of `docs/` are unchanged apart from the front page text; verified with `cargo test --test docs`
- [x] 5.2 The whole suite passes except `tests/udp.rs` `a_receive_has_a_deadline_with_timeout_and_other_tasks_run`, which depends on the Windows timer resolution (it needs more than 3 ticks of `sleep(5)` in 60 ms) and also fails about one run in five on its own, unrelated to this change; `openspec validate` passes for the change and all specs; the old name appears in no tracked file or file name
