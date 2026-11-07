# Brasa for VS Code

Support for the [Brasa](https://github.com/code4performance/brasa-parser) programming language (`.br` files):

- **Syntax highlighting**, brackets and comments.
- **Syntax errors as you type**, from the language server (`brasa-parse --lsp`): squiggles and the Problems panel.
- **Format Document** and format on save, in the standard style (the same as `brasa-parse --fmt`).

## Install

You need the `brasa-parse` compiler (`cargo build --release` in the repository) on your `PATH`, or its full path in the setting below.

Build and install the extension from this folder:

```sh
npx @vscode/vsce package          # writes brasa-lang-<version>.vsix
code --install-extension brasa-lang-0.2.0.vsix
```

The extension has no npm dependencies and needs no build step.

## Settings

| Setting | Default | |
|---|---|---|
| `brasa.server.path` | `brasa-parse` | The executable that runs `brasa-parse --lsp`. A name is looked up on `PATH`. |
| `brasa.server.enabled` | `true` | Turn the language server off (highlighting stays). |
| `brasa.trace` | `false` | Log the messages exchanged with the server to the **Brasa Language Server** output channel. |

Command: **Brasa: Restart Language Server**.

If the executable cannot be started you get one message that names it and a button that opens the setting; highlighting keeps working. In an untrusted workspace the extension does not start an executable named by the workspace's settings.

## Develop

```sh
node --test editors/vscode/test/client.test.js     # needs a built target/debug/brasa-parse (or $BRASA_PARSE)
```

The protocol client is in `client.js` and does not import `vscode`; `extension.js` connects it to the editor.
