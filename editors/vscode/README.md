<img src="icon.png" alt="Zila Lang" width="72" height="72">

# Zila Lang for VS Code

A small, readable language that compiles straight to machine code.

Support for the [Zila Lang](https://github.com/code4performance/zila) programming language (`.zila` files):

- **Syntax highlighting**, brackets and comments.
- **Syntax errors as you type**, from the language server (`zila --lsp`): squiggles and the Problems panel.
- **Format Document** and format on save, in the standard style (the same as `zila --fmt`).

## Install

You need the `zila` compiler (`cargo build --release` in the repository) on your `PATH`, or its full path in the setting below.

Build and install the extension from this folder:

```sh
npx @vscode/vsce package          # writes zila-lang-<version>.vsix
code --install-extension zila-lang-0.5.0.vsix
```

The extension has no npm dependencies and needs no build step.

A copy installed by hand (copied into `~/.vscode/extensions`) must be refreshed by copying the files again. Since 0.5.0 the default command is `zila`; a `zila.server.path` setting that still names the old command (the one before the rename) is not changed for you and fails with the "cannot start" message.

## Settings

| Setting | Default | |
|---|---|---|
| `zila.server.path` | `zila` | The executable that runs `zila --lsp`. A name is looked up on `PATH`. |
| `zila.server.enabled` | `true` | Turn the language server off (highlighting stays). |
| `zila.trace` | `false` | Log the messages exchanged with the server to the **Zila Lang Language Server** output channel. |

Command: **Zila Lang: Restart Language Server**.

If the executable cannot be started you get one message that names it and a button that opens the setting; highlighting keeps working. In an untrusted workspace the extension does not start an executable named by the workspace's settings.

## Develop

```sh
node --test editors/vscode/test/client.test.js     # needs a built target/debug/zila (or $ZILA_BIN)
```

The protocol client is in `client.js` and does not import `vscode`; `extension.js` connects it to the editor.
