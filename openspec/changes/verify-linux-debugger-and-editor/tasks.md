## 1. Debugger on Linux

- [ ] 1.1 On a Linux host (or WSL with Rust), build the crate and fix what the compiler finds in `src/debugger/linux.rs`. Verify: `cargo build` and `cargo clippy --all-targets` are clean on Linux.
- [ ] 1.2 Run `cargo test --test debugger_linux`. Verify: it passes (breakpoint, step, run to exit with the output, pause and kill, a crash reported as signal 11).
- [ ] 1.3 Run `cargo test --test dap` on Linux. Verify: the scripted session passes (breakpoint with moved line, stepping in order, variables of every kind, abort, the wrong-target message for a Windows program); adjust the tests that name Windows-specific text (the crash description) if they fail for that reason only.
- [ ] 1.4 Update the `Platforms` text of the README and `docs/editors.html` (Windows only until now) and the debugger line of the CHANGELOG. Verify: `cargo test --test docs` passes.

## 2. The extension in an editor

- [ ] 2.1 Package with `npx @vscode/vsce package` in `editors/vscode` and install with `code --install-extension zila-lang-0.5.0.vsix`; reload the window. Verify by hand: in a `.zila` file, `.` after a record value offers its fields and methods; hover, go to definition and rename work.
- [ ] 2.2 Press F5 in a `.zila` file with nothing configured. Verify by hand: the program is built, stops at a breakpoint set in the editor, the Variables view shows the locals, step over / into / out move through the lines, and the debug console evaluates a name.
- [ ] 2.3 Repeat 2.1 and 2.2 in Antigravity with `antigravity-ide --install-extension zila-lang-0.5.0.vsix`. Verify by hand: the same results after a window reload.

## 3. Close

- [ ] 3.1 Run `openspec validate verify-linux-debugger-and-editor` and the test targets that do not open sockets. Verify: both pass.
