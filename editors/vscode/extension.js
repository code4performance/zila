// The Zila Lang extension: the language server's syntax errors and formatting in VS Code.
//
// The protocol work is in client.js (which does not depend on VS Code and is tested with
// plain Node). This file connects it to the editor: documents, diagnostics, formatting,
// settings, commands and the output channel.

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

function showDiagnostics(uri, list) {
  const items = list.map((d) => {
    const item = new vscode.Diagnostic(toRange(d.range), d.message, vscode.DiagnosticSeverity.Error);
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
