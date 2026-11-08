## 1. Site scaffold

- [x] 1.1 Create `docs/` directory with `index.html`, `syntax.html`, `examples.html`, `getting-started.html`, and `styles.css`; verify each page opens correctly via `file://` in a browser with the shared stylesheet applied
- [x] 1.2 Build a shared navigation header/footer (linking Overview, Syntax, Examples, Getting Started) and include it on every page; verify every page shows the same nav and each link loads the right page

## 2. Overview page (`index.html`)

- [x] 2.1 Write the landing page content: what Zila Lang is, its design goals, and who it's for, drawing from `README.md`; verify a first-time reader can state Zila Lang's purpose after reading the page alone

## 3. Syntax reference (`syntax.html`)

- [x] 3.1 Document packages/`use`, `record`/`union`/`enum`/`interface` type declarations, and generics, each with a short code sample drawn from `zila.ebnf` and `examples/types.br`
- [x] 3.2 Document functions (including `ref` params, default params, varargs with `...`), `var`/`let` bindings, and control flow (`if`, `match`, `for`, `while`), each with a short code sample drawn from `examples/types.br` and `examples/hello.br`
- [x] 3.3 Document error handling (`!` return types, `?` propagation, `defer`), string interpolation, and attributes (`#[...]`), each with a short code sample drawn from `examples/rest_api.br` and `examples/types.br`
- [x] 3.4 Document `extern`/`asm` FFI blocks with a short code sample drawn from `examples/ffi.br` and `zila-ffi.md`
- [x] 3.5 Cross-check every documented construct against `zila.ebnf` and verify none contradicts the current grammar

## 4. Annotated examples (`examples.html`)

- [x] 4.1 Add an annotated walkthrough of `examples/hello.br` (source + explanation of the features it demonstrates); verify the embedded source matches the file in `examples/`
- [x] 4.2 Add an annotated walkthrough of `examples/rest_api.br`; verify the embedded source matches the file in `examples/`
- [x] 4.3 Add an annotated walkthrough of `examples/types.br`; verify the embedded source matches the file in `examples/`
- [x] 4.4 (added beyond the minimum) Add an annotated excerpt walkthrough of `examples/ffi.br`, cross-linked from the syntax guide's FFI section, matching the proposal's "FFI/low-level" example

## 5. Getting started guide (`getting-started.html`)

- [x] 5.1 Document build/run instructions (`cargo build --release`, `cargo test`) and the `zila-parse` CLI options `--ast`, `--tokens`, and `--run`, drawn from `README.md`; verify each documented command matches what `README.md` and `src/main.rs` actually support

## 6. Final review

- [x] 6.1 Open the site from disk end-to-end (no server) and click through every internal navigation link; verify no broken links and all pages render with styling
- [x] 6.2 Re-read `syntax.html` and `examples.html` against `zila.ebnf` and `examples/*.br` one more time to confirm no drift was introduced while writing content
