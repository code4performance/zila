## Why

`add-editor-intelligence` (archived) delivered the language server features, `--debug`, the debug adapter on Windows and the extension code, but two things could not be verified on the machine it was built on: the debugger on Linux, whose `ptrace` backend is written but was never compiled (WSL there has no Rust toolchain), and the extension in a real editor (VS Code or Antigravity). Until they are, the specification cannot claim that the adapter works on Linux or that F5 and completion work in an editor.

## What Changes

- **Linux debugger**: build and fix `src/debugger/linux.rs`, run `tests/debugger_linux.rs` and the scripted adapter session (`tests/dap.rs`) on Linux, and widen the `Platforms` requirement of the `debugger` capability to the host operating system.
- **Extension in an editor**: install the `.vsix` in VS Code and in Antigravity with each editor's own `--install-extension`, and check by hand that completion appears after `.` and that a breakpoint stops. Then the requirements `Language features` and `Debugging` of the `vscode-extension` capability are added.
- No change to the language, the compiler output or the protocol.

## Capabilities

### Modified Capabilities
- `debugger`: `Platforms` covers Linux hosts too.

### New Requirements
- `vscode-extension`: `Language features` and `Debugging` (code and tests exist; they were left out of the first change because they were not tried in an editor).

## Impact

- `src/debugger/linux.rs` (and whatever compiling it on Linux finds), `tests/debugger_linux.rs`, `tests/dap.rs`; `editors/vscode/` unchanged unless the manual check finds a fault.
- Needs a Linux host or WSL with Rust, and VS Code or Antigravity on a machine with a display.
