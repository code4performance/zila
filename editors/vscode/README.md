<img src="icon.png" alt="Zila Lang" width="72" height="72">

# Zila Lang for VS Code

A small, readable language that compiles straight to machine code.

Support for the [Zila Lang](https://github.com/code4performance/zila) programming language (`.zila` files):

- **Syntax highlighting**, brackets and comments.
- **Errors as you type**, from the language server (`zila --lsp`): syntax errors, then the type errors, unknown names and warnings of the compiler, on the expression they are about, with the unsaved text of every open file.
- **Format Document** and format on save, in the standard style (the same as `zila --fmt`).
- **Completion** (names in scope, fields and methods after `.`, package members, attributes), **hover** (type and doc comment), **signature help**, **go to definition** (also into the standard library), **find references**, **outline and workspace symbols**, and **rename** across the files of the project.
- **Debugging** (`zila --dap`): press F5 in a `.zila` file. Breakpoints, continue, pause, step over / into / out, the call stack, the Variables view (records, lists, maps and enums expanded), the debug console, and stopping when the program aborts.

## Install

You need the `zila` compiler (`cargo build --release` in the repository) on your `PATH`, or its full path in the setting below.

Build and install the extension from this folder:

```sh
npx @vscode/vsce package                                    # writes zila-lang-<version>.vsix
code --install-extension zila-lang-0.5.0.vsix               # VS Code
antigravity-ide --install-extension zila-lang-0.5.0.vsix    # Antigravity
```

Use the editor's **own** `--install-extension`: VS Code and the editors built on it (Antigravity and others) keep separate extension folders, and files copied into one of them by hand are never registered. Reload the window after installing (Developer: Reload Window).

The extension has no npm dependencies and needs no build step.

A copy installed by hand (copied into `~/.vscode/extensions`) must be refreshed by copying the files again. Since 0.5.0 the default command is `zila`; a `zila.server.path` setting that still names the old command (the one before the rename) is not changed for you and fails with the "cannot start" message.

## Settings

| Setting | Default | |
|---|---|---|
| `zila.server.path` | `zila` | The executable that runs `zila --lsp`. A name is looked up on `PATH`. |
| `zila.server.enabled` | `true` | Turn the language server off (highlighting stays). |
| `zila.trace` | `false` | Log the messages exchanged with the server to the **Zila Lang Language Server** output channel. |

Commands: **Zila Lang: Restart Language Server** and **Zila Lang: Debug Current File**.

## Debugging

Open a `.zila` file and press F5 (or choose *Zila Lang* in Run and Debug). With nothing configured the file in the editor is debugged. A `launch.json` entry:

```json
{
  "type": "zila",
  "request": "launch",
  "name": "Debug current file",
  "program": "${file}",
  "args": [],
  "cwd": "${workspaceFolder}",
  "stopOnEntry": false
}
```

The adapter is `zila --dap` (the same executable as the language server, so `zila.server.path` applies): it builds the program with `--debug`, runs it under the debugger and reports the build error if there is one. A breakpoint on a line without code moves to the next line that has code. Values the optimizer kept in a register are only readable in the innermost frame and are shown as `unavailable` above it. Programs are debugged on the operating system they were built for (Windows, and Linux through `ptrace`).

If the executable cannot be started you get one message that names it and a button that opens the setting; highlighting keeps working. In an untrusted workspace the extension does not start an executable named by the workspace's settings.

## Develop

```sh
node --test editors/vscode/test/client.test.js editors/vscode/test/extension.test.js   # needs a built target/debug/zila (or $ZILA_BIN)
```

The protocol client is in `client.js` and does not import `vscode`; `extension.js` connects it to the editor.
