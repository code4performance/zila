## Why

`ui` (native windows) and `ws` (Winsock) are the last libraries that exist only on Windows, so a program that opens a window cannot be built with `--target linux`. Everything else in the language already behaves the same on both systems. `ws` is also an odd case: it is an internal helper of `net`, but user programs can `use ws` directly, which is a Windows-only way of programming sockets that has no Linux equivalent.

## What Changes

- **`use ui` works on Linux** with the same functions, widgets, events and layout as on Windows: `Window`, `TextBox`, `Button`, `ListView`, `Label`, `vstack`, `hstack`, the properties `text`, `count` and `selected`, `add`, `remove`, `clear`, `onClick`, `onChange`, `onSelect`, `layout`, `show`, `close`, `run` and `alert`. `examples/ui.br` builds and runs unchanged for both targets.
- **How it is built on Linux:** a new `std/ui.linux.br`, written in Brasa, talks the X11 protocol directly over the X server's Unix socket (no C library, no GTK, no linker; the program stays a static executable). X11 has no widgets, so the Linux layer draws and handles the controls itself (text box with caret and editing keys, button, list with selection and scrolling, label, modal alert) and arranges them with the same layout rules as Windows. Wayland-only sessions work through XWayland.
- **One way to program, on both systems.** Anything Windows-shaped in the public surface is changed so both layers expose exactly the same thing: the handle field of a control stops being called `hwnd` (**BREAKING** only for programs that read it, which the docs never documented), `ui.run()` has one documented contract, and a program that cannot reach a display fails with the same error text.
- **`ws` becomes internal on both systems.** `use ws` in a user program is rejected on every target with a message that says it is internal to `net`; `net` keeps using it on Windows. (**BREAKING** for programs that imported `ws` directly.)
- **Same checks on both systems.** A conformance test builds one set of programs for Windows and for Linux and drives them with synthetic input (clicks, typing, selection, close), comparing what the program printed.
- Documentation: the compatibility table, the `ui` and limits pages, and the roadmap row (a window toolkit for Linux is done; macOS stays).

## Capabilities

### New Capabilities

- `ui-library`: the windowing library as one contract for Windows and Linux (widgets, layout, events, run loop, alert, display errors, scripted input for tests).

### Modified Capabilities

- `linux-target`: `ui` is available on Linux; `ws` is internal everywhere.
- `linux-parity`: `ui` joins the libraries that build and behave the same on both targets.

## Impact

- New `std/ui.linux.br`; `std/ui.br` (rename the handle field, shared event contract, display-error text, scripted input); `src/codegen_x64.rs` (Linux layer for `ui`, `ws` rejected for user code); `std/task.linux.br` only if the `ui` loop needs the epoll descriptor.
- `tests/ui.rs` (new; conformance on Windows, and on Linux through WSL with the X server of WSLg, skipped when neither is available); `tests/linux.rs` and `tests/codegen.rs` (rejection tests updated).
- `docs/native.html`, `docs/concurrency.html` (limits), `docs/stdlib.html`, README.
