## 1. Process rule

- [x] 1.1 Add a docs-sync rule to `openspec/config.yaml` (under `rules.tasks`) requiring every change to include a docs task or record why none is needed; verify with `openspec instructions tasks --change keep-docs-in-sync --json` that the rule is listed

## 2. Drift checks

- [x] 2.1 Create `tests/docs.rs` with helpers that read `docs/*.html`, strip tags and decode entities; verify with a unit test of the helper on a small highlighted snippet
- [x] 2.2 Add the example-drift test (every `<pre>` marked `<!-- example: examples/NAME.br -->` equals that file); verify it passes now and fails after temporarily editing an example, then restore it
- [x] 2.3 Expose the CLI option table (`FLAGS`) from `src/main.rs`, use it for argument parsing, and add the flags test comparing it with `docs/getting-started.html`; verify with `cargo test` and by temporarily adding a flag
- [x] 2.4 Add the keywords test (every entry of `zila::lexer::KEYWORDS` appears in `docs/syntax.html`, and no removed keyword such as `union` is shown as one); verify it passes and fails when a keyword is temporarily dropped from the page
- [x] 2.5 Add the site-integrity test (same nav on every page, all internal links and anchors resolve, every `docs/*.html` is linked); verify it passes and fails for a deliberately broken link

## 3. Example generator

- [x] 3.1 Implement `zila-parse --docs` to rewrite marked example blocks from `examples/*.br` with lexer-based highlighting; verify running it twice is idempotent (the second run changes nothing) and the drift test passes afterwards
- [x] 3.2 Add `<!-- example: ... -->` markers to the existing example blocks in `docs/examples.html` and `docs/syntax.html` and regenerate them; verify `cargo test` passes

## 4. Bring the site up to date

- [x] 4.1 Update `docs/getting-started.html` with `--build`, `-o`, `--asm`, `--os`, the Visual Studio requirement, and the supported native subset; verify the flags test passes and each command runs as written
- [x] 4.2 Add `docs/native.html` covering native compilation, the memory model (reference counting, cycles leak, `live_objects()`), `or error`/`try`, enums and `match`, and the "not yet supported" list (floats, generics, maps, `defer`); verify every code sample compiles with `zila-parse -o`
- [x] 4.3 Add `docs/concurrency.html` covering `spawn`, `channel(T)`, `send`/`receive`/`close`, cooperative scheduling, deadlock detection and limits; verify the sample builds and runs
- [x] 4.4 Add `docs/os.html` covering the boot sequence, `os/boot.br`, `os/kernel.br`, the shell commands, building the image, running it in QEMU (`os/run.ps1`) and the 16-bit limitation of the boot sector; verify the commands work and the page is linked in the nav
- [x] 4.5 Update `docs/index.html` and `docs/syntax.html` for the new syntax (`enum` with data, `or error`, `try`, lambdas without `->`, `int`/`float`/`byte`) and the `ui` library; verify the keywords test passes and no page mentions `union`, `T!` or postfix `?`
- [x] 4.6 Run the whole suite and open each page from disk; verify `cargo test` passes and every nav link works without a server
