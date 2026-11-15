// The Zila Lang extension: the language server's diagnostics, formatting, completion, hover,
// signature help, definition, references, symbols and rename in VS Code, and the debugger.
//
// The protocol work is in client.js (which does not depend on VS Code and is tested with
// plain Node). This file connects it to the editor: documents, diagnostics, the providers,
// settings, commands, the output channel and the `zila` debugger (`zila --dap`).

'use strict';

const vscode = require('vscode');
const { Client } = require('./client');

let client = null;
let diagnostics = null;
let output = null;
let warnedMissing = false;

function settings() {
  return vscode.workspace.getConfiguration('zila');
}

function isZila(doc) {
  return doc.languageId === 'zila' && (doc.uri.scheme === 'file' || doc.uri.scheme === 'untitled');
}

function toRange(r) {
  return new vscode.Range(r.start.line, r.start.character, r.end.line, r.end.character);
}

function toPosition(p) {
  return new vscode.Position(p.line, p.character);
}

/** The severity of an LSP diagnostic (1 error, 2 warning, 3 information, 4 hint). */
function severity(n) {
  const s = vscode.DiagnosticSeverity;
  return n === 2 ? s.Warning : n === 3 ? s.Information : n === 4 ? s.Hint : s.Error;
}

function showDiagnostics(uri, list) {
  const items = list.map((d) => {
    const item = new vscode.Diagnostic(toRange(d.range), d.message, severity(d.severity));
    item.source = d.source || 'zila';
    return item;
  });
  diagnostics.set(vscode.Uri.parse(uri), items);
}

async function startServer() {
  if (!settings().get('server.enabled', true)) return;
  const command = settings().get('server.path', 'zila') || 'zila';
  const trace = settings().get('trace', false);
  warnedMissing = false;
  client = new Client({
    command,
    log: trace ? (line) => output.appendLine(line) : undefined,
    onDiagnostics: showDiagnostics,
    onError: (e) => {
      output.appendLine('cannot start ' + command + ': ' + e.message);
      if (warnedMissing) return;
      warnedMissing = true;
      vscode.window
        .showErrorMessage(
          'Zila Lang: cannot start the language server "' + command + '" (' + e.message + '). Highlighting still works; set zila.server.path to the zila executable.',
          'Open Settings'
        )
        .then((choice) => {
          if (choice === 'Open Settings') vscode.commands.executeCommand('workbench.action.openSettings', 'zila.server.path');
        });
    },
    onStopped: () => {
      vscode.window.showErrorMessage('Zila Lang: the language server stopped after exiting several times. Use "Zila Lang: Restart Language Server" to try again.');
    },
  });
  for (const doc of vscode.workspace.textDocuments) {
    if (isZila(doc)) client.open(doc.uri.toString(), doc.getText(), doc.version);
  }
  await client.start();
}

async function stopServer() {
  const old = client;
  client = null;
  if (diagnostics) diagnostics.clear();
  if (old) await old.stop();
}

async function restartServer() {
  await stopServer();
  await startServer();
}

/** Brings the server up to date with the document, then asks. */
async function ask(method, document, params, raw) {
  if (!client) return undefined;
  const uri = document.uri.toString();
  client.change(uri, document.getText(), document.version, true);
  return client.query(method, uri, params, raw);
}

function toLocation(l) {
  return new vscode.Location(vscode.Uri.parse(l.uri), toRange(l.range));
}

/** The kinds of the protocol are those of the editor plus one, for the ones the server uses. */
const kind = (k) => (typeof k === 'number' && k > 0 ? k - 1 : 0);

function toSymbol(s) {
  const symbol = new vscode.DocumentSymbol(s.name, s.detail || '', kind(s.kind), toRange(s.range), toRange(s.selectionRange));
  symbol.children = (s.children || []).map(toSymbol);
  return symbol;
}

function languageProviders() {
  const L = vscode.languages;
  return [
    L.registerCompletionItemProvider(
      'zila',
      {
        async provideCompletionItems(document, position) {
          const r = await ask('textDocument/completion', document, { position });
          if (!r || !Array.isArray(r.items)) return [];
          return r.items.map((i) => {
            const item = new vscode.CompletionItem(i.label, kind(i.kind));
            if (i.detail) item.detail = i.detail;
            return item;
          });
        },
      },
      '.'
    ),
    L.registerHoverProvider('zila', {
      async provideHover(document, position) {
        const r = await ask('textDocument/hover', document, { position });
        if (!r || !r.contents) return undefined;
        return new vscode.Hover(new vscode.MarkdownString(r.contents.value), r.range ? toRange(r.range) : undefined);
      },
    }),
    L.registerSignatureHelpProvider(
      'zila',
      {
        async provideSignatureHelp(document, position) {
          const r = await ask('textDocument/signatureHelp', document, { position });
          if (!r || !Array.isArray(r.signatures)) return undefined;
          const help = new vscode.SignatureHelp();
          help.signatures = r.signatures.map((s) => {
            const info = new vscode.SignatureInformation(s.label, s.documentation);
            info.parameters = (s.parameters || []).map((p) => new vscode.ParameterInformation(p.label));
            return info;
          });
          help.activeSignature = r.activeSignature || 0;
          help.activeParameter = r.activeParameter || 0;
          return help;
        },
      },
      '(',
      ','
    ),
    L.registerDefinitionProvider('zila', {
      async provideDefinition(document, position) {
        const r = await ask('textDocument/definition', document, { position });
        return r ? toLocation(r) : undefined;
      },
    }),
    L.registerReferenceProvider('zila', {
      async provideReferences(document, position, context) {
        const r = await ask('textDocument/references', document, { position, context: { includeDeclaration: !!(context && context.includeDeclaration) } });
        return Array.isArray(r) ? r.map(toLocation) : [];
      },
    }),
    L.registerDocumentSymbolProvider('zila', {
      async provideDocumentSymbols(document) {
        const r = await ask('textDocument/documentSymbol', document, {});
        return Array.isArray(r) ? r.map(toSymbol) : [];
      },
    }),
    L.registerWorkspaceSymbolProvider({
      async provideWorkspaceSymbols(query) {
        if (!client) return [];
        const r = await client.request('workspace/symbol', { query });
        if (!Array.isArray(r)) return [];
        return r.map((s) => new vscode.SymbolInformation(s.name, kind(s.kind), s.containerName || '', toLocation(s.location)));
      },
    }),
    L.registerRenameProvider('zila', {
      async provideRenameEdits(document, position, newName) {
        const r = await ask('textDocument/rename', document, { position, newName }, true);
        if (!r) return undefined;
        // the server says why a rename cannot be done (a name that is taken, a name of the standard library)
        if (r.error) throw new Error(r.error.message);
        const edit = new vscode.WorkspaceEdit();
        const changes = (r.result && r.result.changes) || {};
        for (const uri of Object.keys(changes)) {
          for (const e of changes[uri]) edit.replace(vscode.Uri.parse(uri), toRange(e.range), e.newText);
        }
        return edit;
      },
    }),
  ];
}

/** What starting a debug session with nothing configured means: the file in the editor. */
function resolveDebugConfiguration(folder, config) {
  if (!config.type && !config.request && !config.name) {
    const editor = vscode.window.activeTextEditor;
    if (!editor || editor.document.languageId !== 'zila') return undefined;
    return { type: 'zila', request: 'launch', name: 'Debug current file', program: editor.document.fileName, stopOnEntry: false };
  }
  if (config.request === 'launch' && !config.program) {
    const editor = vscode.window.activeTextEditor;
    if (editor && editor.document.languageId === 'zila') config.program = editor.document.fileName;
  }
  return config;
}

function debugProviders() {
  return [
    vscode.debug.registerDebugConfigurationProvider('zila', { resolveDebugConfiguration }),
    // the adapter is the same executable as the language server, with `--dap`
    vscode.debug.registerDebugAdapterDescriptorFactory('zila', {
      createDebugAdapterDescriptor() {
        const command = settings().get('server.path', 'zila') || 'zila';
        return new vscode.DebugAdapterExecutable(command, ['--dap']);
      },
    }),
    vscode.commands.registerCommand('zila.debugFile', () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor || editor.document.languageId !== 'zila') return undefined;
      return vscode.debug.startDebugging(undefined, { type: 'zila', request: 'launch', name: 'Debug current file', program: editor.document.fileName });
    }),
  ];
}

function activate(context) {
  output = vscode.window.createOutputChannel('Zila Lang Language Server');
  diagnostics = vscode.languages.createDiagnosticCollection('zila');
  context.subscriptions.push(output, diagnostics);

  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument((doc) => {
      if (client && isZila(doc)) client.open(doc.uri.toString(), doc.getText(), doc.version);
    }),
    vscode.workspace.onDidChangeTextDocument((e) => {
      if (client && isZila(e.document)) client.change(e.document.uri.toString(), e.document.getText(), e.document.version);
    }),
    vscode.workspace.onDidCloseTextDocument((doc) => {
      if (client && isZila(doc)) client.close(doc.uri.toString());
    }),
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration('zila.server') || e.affectsConfiguration('zila.trace')) restartServer();
    }),
    vscode.commands.registerCommand('zila.restartServer', restartServer),
    ...languageProviders(),
    ...debugProviders(),
    vscode.languages.registerDocumentFormattingEditProvider('zila', {
      async provideDocumentFormattingEdits(document) {
        if (!client) return [];
        const uri = document.uri.toString();
        client.change(uri, document.getText(), document.version, true);
        const edits = await client.format(uri);
        if (!edits) return [];
        return edits.map((e) => vscode.TextEdit.replace(toRange(e.range), e.newText));
      },
    })
  );

  return startServer();
}

async function deactivate() {
  await stopServer();
}

module.exports = { activate, deactivate };
