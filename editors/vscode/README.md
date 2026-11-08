# Zila Lang for VS Code

Support for the [Zila Lang](https://github.com/code4performance/zila) programming language (`.br` files):

- **Syntax highlighting**, brackets and comments.
- **Syntax errors as you type**, from the language server (`zila-parse --lsp`): squiggles and the Problems panel.
- **Format Document** and format on save, in the standard style (the same as `zila-parse --fmt`).

## Install

You need the `zila-parse` compiler (`cargo build --release` in the repository) on your `PATH`, or its full path in the setting below.

Build and install the extension from this folder:

```sh
npx @vscode/vsce package          # writes zila-lang-<version>.vsix
code --install-extension zila-lang-0.3.0.vsix
```

The extension has no npm dependencies and needs no build step.

## Settings

| Setting | Default | |
|---|---|---|
| `zila.server.path` | `zila-parse` | The executable that runs `zila-parse --lsp`. A name is looked up on `PATH`. |
| `zila.server.enabled` | `true` | Turn the language server off (highlighting stays). |
| `zila.trace` | `false` | Log the messages exchanged with the server to the **Zila Lang Language Server** output channel. |

Command: **Zila Lang: Restart Language Server**.

If the executable cannot be started you get one message that names it and a button that opens the setting; highlighting keeps working. In an untrusted workspace the extension does not start an executable named by the workspace's settings.

## Develop

```sh
node --test editors/vscode/test/client.test.js     # needs a built target/debug/zila-parse (or $ZILA_PARSE)
```

The protocol client is in `client.js` and does not import `vscode`; `extension.js` connects it to the editor.

## Renamed from Zila Lang

From version 0.3.0 the extension, its language id and its settings use the name Zila Lang: the settings `zila.server.path`, `zila.server.enabled` and `zila.trace` are now `zila.server.path`, `zila.server.enabled` and `zila.trace`, and the command `zila.restartServer` is now `zila.restartServer`. Settings saved under the old keys are ignored. Formerly Zila Lang.
