## Context

`std/ui.br` (606 lines) is written in Brasa on Win32: real controls (`EDIT`, `BUTTON`, `LISTBOX`, `STATIC`), a window procedure called by Windows, a message loop in `run()`, DPI scaling and its own stack layout (`pref`, `place`, `relayout`). `Control` is a record `{ hwnd, kind, children }`. `ws` is Winsock declarations used only by `net`. On Linux the compiler already builds static ELF programs without libc, has syscalls (`sys.syscall`), epoll-based tasks (`std/task.linux.br`), a Unix-socket-capable `net` layer, and optional dynamic linking for `extern`. Libraries with a Linux layer follow one convention: `std/<name>.linux.br` replaces `std/<name>.br`. A WSL2 machine with WSLg offers an X server (`/tmp/.X11-unix/X0`), which makes Linux `ui` testable here. See proposal.md for motivation.

## Goals / Non-Goals

**Goals**
- The same `ui` source runs on both targets with the same behavior; the public surface is identical by construction and by test.
- The Linux program stays a static executable with no GTK/Qt/Xlib dependency.

**Non-Goals**
- A native look on Linux (the Linux controls are drawn by the library with one fixed, simple style).
- Wayland-native clients, IME/compose input, right-to-left or complex scripts, clipboard, menus, images, custom drawing, multi-line text, more widgets than exist today, screen readers.
- Letting other tasks run inside the `ui` loop (still a documented limit on both targets).
- macOS.

## Decisions

1. **X11 protocol spoken directly, in Brasa.** `ui.linux.br` connects to `/tmp/.X11-unix/X<n>` (display from `DISPLAY`; TCP displays are not supported), does the connection setup (including the `~/.Xauthority` MIT-MAGIC-COOKIE-1 entry when present), and uses core requests only: `CreateWindow`, `MapWindow`, `ChangeProperty` (title, `WM_PROTOCOLS`/`WM_DELETE_WINDOW`), `CreateGC`, `PolyFillRectangle`, `PolyRectangle`, `PolyLine`, `ImageText8`/`OpenFont` (server-side core font), `GetKeyboardMapping` for key translation, and events `Expose`, `ButtonPress`, `KeyPress`, `ConfigureNotify`, `ClientMessage`. *Alternatives:* GTK3 through `extern` (native look, but a dynamic dependency on a large library, no GTK in the test environment, callbacks and struct layouts to maintain); Wayland (needs shared memory, protocol objects, and a compositor-specific shell). X11 core has the smallest, stable surface, works under XWayland and WSLg, and keeps "no C glue".
2. **Draw the widgets in the library.** X11 has no controls, so each `Control` kind has `paint` and `handle_event` functions: text box (text, caret, selection by caret only, no mouse selection), button (pressed/hover-less), list (items, selected row, scroll offset, wheel buttons 4/5), label. Text is drawn with a core font; width is measured with `QueryTextExtents` (fixed font fallback of 6x13 if the server has none) so `pref` can compute sizes.
3. **Shared model, two backends.** The widget tree, layout rules (`PAD`, `GAP`, preferred sizes, remaining space for lists), handler table, `run()` contract and the scripted-input interpreter are the same logic on both targets. To keep them equal, the platform-independent code moves out of `ui.br` into a section that is textually identical in both files and is verified identical by a test (`tests/ui.rs` extracts the region between `// ---- shared ----` markers and compares). A single shared file is not used because the library loader supports only one replacement file per library; adding an "include" mechanism would be a language change outside this proposal.
4. **No platform names in the public API.** `Control.hwnd` becomes `Control.id` (an opaque `usize`). Public functions keep their signatures. A test compares the public declarations of the two layers.
5. **DPI.** Windows asks the window; Linux reads `Xft.dpi` from the root window's `RESOURCE_MANAGER` property, else computes from the screen's millimeter size, else 96. The same `scale(v, dpi)` function applies.
6. **Event loop.** `run()` on Linux blocks in `read` on the X socket (non-blocking socket parked with `task.wait_fd` is not used, because tasks are not run inside the loop on either target), processes all queued events, repaints exposed controls, and returns when the last window's `WM_DELETE_WINDOW` or `close()` has destroyed it.
7. **Alert.** A modal child window with a label and an OK button on both targets (Windows keeps `MessageBox`; on Linux it is a transient window with its own mini loop). Enter and Escape dismiss it.
8. **Display failure.** One text on both targets: `ui: cannot open a display: <reason>`; raised as a panic-style error at the first call that needs the display (Windows effectively never fails, but the function that raises it is shared).
9. **Scripted input.** `BRASA_UI_SCRIPT` is read by the shared code; the steps are applied by calling the same handlers the real events call (`click` → `fire`, `type`/`key` → the text box editor, `select` → list selection). This gives deterministic, display-independent conformance tests, and a separate smoke test checks that real X events reach the handlers (it sends events with `XTEST`-free `SendEvent` from the test's own X client, or skips if no X server).
10. **`ws` is internal.** The loader rejects `use ws` from a user file (not from a library file) on every target with "`ws` is internal to `net`". `net` on Windows keeps importing it.

## Risks / Trade-offs

- [Self-drawn controls look different from Windows' native ones] → documented; the layout and behavior are identical, the look is not a stated requirement.
- [X authority parsing and the X11 wire format are easy to get wrong] → small, table-driven request builders; tested against the real WSLg server and against a recorded handshake.
- [Core fonts may be missing on a minimal X server] → fall back to the server's `fixed` font, then to drawing with block rectangles is NOT done; the call fails with the display error if no font can be opened.
- [Duplicated shared code can drift] → the identical-region test.
- [WSLg is the only X server in CI here] → tests skip without a display; the scripted tests still run on Windows.
- [Renaming `hwnd` breaks programs that read it] → undocumented field; called out as BREAKING.

## Migration Plan

Existing Windows programs keep working unless they read `Control.hwnd` or `use ws`. Both are called out in the docs. No data migration.

## Open Questions

- Whether to offer TCP X displays (`DISPLAY=host:0`) later; the spec only requires local sockets.
