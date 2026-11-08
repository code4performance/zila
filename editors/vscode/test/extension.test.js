// Tests of extension.js with a stand-in for the `vscode` module, against the real
// `zila --lsp`. Run with: node --test editors/vscode/test/extension.test.js

'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const os = require('os');
const Module = require('module');

function binary() {
  if (process.env.ZILA_BIN) return process.env.ZILA_BIN;
  const root = path.join(__dirname, '..', '..', '..');
  for (const dir of ['debug', 'release']) {
    for (const name of ['zila.exe', 'zila']) {
      const p = path.join(root, 'target', dir, name);
      if (fs.existsSync(p)) return p;
    }
  }
  return null;
}

const exe = binary();
const skip = exe ? false : 'zila is not built (cargo build)';
const GOOD = 'package main\n\nfunc main()\nend\n';
const BAD = 'package main\n\nfunc main()\n    let = 3\nend\n';
const UNFORMATTED = 'package main\n\nfunc main()\nprintln(1,2)\nend\n';

/** A minimal `vscode` that records what the extension does. */
function fakeVscode(config) {
  const log = { diagnostics: new Map(), errors: [], commands: new Map(), executed: [], handlers: {}, provider: null, output: [] };
  const handler = (name) => (fn) => {
    log.handlers[name] = fn;
    return { dispose() {} };
  };
  const vscode = {
    log,
    config,
    workspace: {
      textDocuments: [],
      getConfiguration: () => ({ get: (key, def) => (key in vscode.config ? vscode.config[key] : def) }),
      onDidOpenTextDocument: handler('open'),
      onDidChangeTextDocument: handler('change'),
      onDidCloseTextDocument: handler('close'),
      onDidChangeConfiguration: handler('config'),
    },
    window: {
      createOutputChannel: () => ({ appendLine: (l) => log.output.push(l), dispose() {} }),
      showErrorMessage: (message, ...buttons) => {
        log.errors.push({ message, buttons });
        return Promise.resolve(undefined);
      },
    },
    languages: {
      createDiagnosticCollection: () => ({
        set: (uri, items) => log.diagnostics.set(uri.toString(), items),
        clear: () => log.diagnostics.clear(),
        dispose() {},
      }),
      registerDocumentFormattingEditProvider: (language, provider) => {
        log.provider = provider;
        return { dispose() {} };
      },
    },
    commands: {
      registerCommand: (name, fn) => {
        log.commands.set(name, fn);
        return { dispose() {} };
      },
      executeCommand: (...args) => {
        log.executed.push(args);
        return Promise.resolve();
      },
    },
    Range: class {
      constructor(a, b, c, d) {
        this.start = { line: a, character: b };
        this.end = { line: c, character: d };
      }
    },
    Diagnostic: class {
      constructor(range, message, severity) {
        this.range = range;
        this.message = message;
        this.severity = severity;
      }
    },
    DiagnosticSeverity: { Error: 0 },
    TextEdit: { replace: (range, newText) => ({ range, newText }) },
    Uri: { parse: (s) => ({ toString: () => s }) },
  };
  return vscode;
}

/** A document the way VS Code hands it to the extension. */
function doc(uri, text, version) {
  return {
    languageId: 'zila',
    uri: { scheme: 'file', toString: () => uri },
    version,
    getText: () => text,
  };
}

/** Loads a fresh extension.js against a fake vscode. */
function load(config) {
  const vscode = fakeVscode(config);
  const original = Module._load;
  Module._load = function (request, ...rest) {
    return request === 'vscode' ? vscode : original.call(this, request, ...rest);
  };
  for (const key of Object.keys(require.cache)) {
    if (key.endsWith('extension.js') || key.endsWith('client.js')) delete require.cache[key];
  }
  const extension = require('../extension.js');
  Module._load = original;
  return { vscode, extension };
}

async function until(fn, what) {
  for (let i = 0; i < 200; i++) {
    if (fn()) return;
    await new Promise((r) => setTimeout(r, 25));
  }
  assert.fail('timed out waiting for ' + what);
}

const context = () => ({ subscriptions: [] });

test('errors appear as you type and clear when fixed; formatting returns edits', { skip }, async () => {
  const { vscode, extension } = load({ 'server.path': exe });
  await extension.activate(context());
  const log = vscode.log;
  assert.ok(log.commands.has('zila.restartServer'));
  assert.ok(log.provider);

  const d = doc('file:///t.zila', BAD, 1);
  log.handlers.open(d);
  await until(() => (log.diagnostics.get('file:///t.zila') || []).length === 1, 'the syntax error');
  const item = log.diagnostics.get('file:///t.zila')[0];
  assert.strictEqual(item.range.start.line, 3);
  assert.strictEqual(item.source, 'zila');

  log.handlers.change({ document: doc('file:///t.zila', GOOD, 2) });
  await until(() => (log.diagnostics.get('file:///t.zila') || []).length === 0, 'the fix');

  const edits = await log.provider.provideDocumentFormattingEdits(doc('file:///t.zila', UNFORMATTED, 3));
  assert.strictEqual(edits.length, 1);
  assert.ok(edits[0].newText.includes('    println(1, 2)'));
  const none = await log.provider.provideDocumentFormattingEdits(doc('file:///t.zila', GOOD, 4));
  assert.deepStrictEqual(none, []);

  log.handlers.close(doc('file:///t.zila', GOOD, 4));
  await extension.deactivate();
});

test('a missing executable gives one message with an Open Settings button and no crash', async () => {
  const missing = path.join(os.tmpdir(), 'no such dir', 'zila');
  const { vscode, extension } = load({ 'server.path': missing });
  await extension.activate(context());
  assert.strictEqual(vscode.log.errors.length, 1);
  assert.ok(vscode.log.errors[0].message.includes(missing));
  assert.deepStrictEqual(vscode.log.errors[0].buttons, ['Open Settings']);
  // highlighting does not depend on the server: the formatting provider just returns nothing
  vscode.log.handlers.open(doc('file:///m.zila', BAD, 1));
  const edits = await vscode.log.provider.provideDocumentFormattingEdits(doc('file:///m.zila', UNFORMATTED, 2));
  assert.deepStrictEqual(edits, []);
  assert.strictEqual(vscode.log.errors.length, 1);
  await extension.deactivate();
});

test('a disabled server starts nothing', async () => {
  const missing = path.join(os.tmpdir(), 'no such dir', 'zila');
  const { vscode, extension } = load({ 'server.path': missing, 'server.enabled': false });
  await extension.activate(context());
  assert.strictEqual(vscode.log.errors.length, 0);
  await extension.deactivate();
});

test('the restart command starts the server again with the current settings', { skip }, async () => {
  const { vscode, extension } = load({ 'server.path': exe });
  await extension.activate(context());
  const log = vscode.log;
  log.handlers.open(doc('file:///r.zila', BAD, 1));
  await until(() => (log.diagnostics.get('file:///r.zila') || []).length === 1, 'the first error');
  vscode.workspace.textDocuments.push(doc('file:///r.zila', BAD, 1));
  await log.commands.get('zila.restartServer')();
  await until(() => (log.diagnostics.get('file:///r.zila') || []).length === 1, 'the error after the restart');
  await extension.deactivate();
});

test('the trace setting writes the protocol to the output channel', { skip }, async () => {
  const { vscode, extension } = load({ 'server.path': exe, trace: true });
  await extension.activate(context());
  assert.ok(vscode.log.output.some((l) => l.startsWith('--> ') && l.includes('"initialize"')));
  await extension.deactivate();
});
