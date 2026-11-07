## 1. Make the existing library platform-neutral

- [x] 1.1 Rename `Control.hwnd` to `Control.id` in `std/ui.br` and everything that reads it; verify with `cargo test --test codegen` (the ui example still compiles) and a grep that no `hwnd` remains in public declarations
- [x] 1.2 Move the platform-independent code (tree, `pref`, layout rules, handler table, `run()` contract) into a `// ---- shared ----` region of `std/ui.br`; verify the Windows `examples/ui.br` still builds and `cargo test --test codegen --test stdlib` pass
- [x] 1.3 Add the shared scripted-input interpreter (`BRASA_UI_SCRIPT`: `click`, `type`, `key`, `select`, `close`) and the shared `ui: cannot open a display: <reason>` error; verify with a Windows test that runs a scripted `examples/ui.br` and checks the order of handler output
- [x] 1.4 Make `ws` internal: reject `use ws` from user files on every target with the "internal to `net`" message; verify with tests for both targets and that `net` on Windows still builds

## 2. The Linux layer

- [x] 2.1 `std/ui.linux.br`: X11 connection (display parsing, Unix socket, `.Xauthority` cookie, setup reply) with a unit-style test that parses a recorded handshake and an integration test that connects to the WSLg server
- [x] 2.2 Windows: create/map/destroy, title, `WM_DELETE_WINDOW`, `ConfigureNotify` resize, DPI; verify by opening a window on WSLg and reading its size and title back with `GetGeometry` and `GetProperty`
- [x] 2.3 Drawing and fonts: GC, rectangles, text with measured width, repaint on `Expose`; verify with a test that draws a label and reads pixels back with `GetImage` at known coordinates
- [x] 2.4 Controls: label, button (press/release/click), text box (typing, Backspace, Delete, arrows, Home, End, Enter-as-default-button), list (items, selection by click and Up/Down, scroll, wheel); verify each with scripted and real-event tests
- [x] 2.5 Layout and the `run()` loop on top of the shared code; `alert` as a modal window; verify that resizing recomputes the arrangement (list grows, text box height constant) and that closing the last window returns from `run()`
- [x] 2.6 Register the Linux layer for `ui` in `src/codegen_x64.rs` (`std_linux_source`, `OS_LIBRARIES`, remove the `ui` rejection); verify `--target linux` builds `examples/ui.br` as a static executable

## 3. Equivalence checks

- [x] 3.1 `tests/ui.rs`: the shared region is byte-identical in both files and the public declarations of both layers are identical; verify the test fails when one file is edited
- [x] 3.2 `tests/ui.rs`: the same scripted programs (add to list, selection, edit keys, close, alert, no display) run on Windows and on Linux (WSL) and print identical output; skipped when the environment lacks a toolchain, WSL or a display
- [x] 3.3 Update `tests/linux.rs` and `tests/codegen.rs`: replace the "ui rejected on Linux" tests with "ui builds on Linux", keep the `ws` rejection; verify the full suite (without `tests/async.rs`)

## 4. Documentation

- [x] 4.1 `docs/native.html` compatibility table (`ui` yes/yes, `ws` internal), `docs/concurrency.html` limits (the `ui` loop note, tasks are no longer Windows-only), `docs/stdlib.html` (`ui` on both, X11 note), README roadmap row and library list; verify with `cargo test --test docs --test linux` (the table test) and a read-through of the pages
- [x] 4.2 Record the BREAKING changes (`Control.hwnd`, `use ws`) in the docs; verify the pages mention both
- [x] 4.3 `openspec validate add-ui-linux` passes; archive after implementation and update `openspec/specs/`
