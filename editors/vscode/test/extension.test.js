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
  const log = { diagnostics: new Map(), errors: [], commands: new Map(), executed: [], handlers: {}, provider: null, output: [], providers: {}, triggers: {}, debug: {}, started: [] };
  const register = (name) => (...args) => {
    // (language, provider, ...triggers) or (provider)
    const provider = typeof args[0] === 'string' ? args[1] : args[0];
    log.providers[name] = provider;
    log.triggers[name] = typeof args[0] === 'string' ? args.slice(2) : args.slice(1);
    return { dispose() {} };
  };
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
      activeTextEditor: undefined,
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
      registerCompletionItemProvider: register('completion'),
      registerHoverProvider: register('hover'),
      registerSignatureHelpProvider: register('signature'),
      registerDefinitionProvider: register('definition'),
      registerReferenceProvider: register('references'),
      registerDocumentSymbolProvider: register('symbols'),
      registerWorkspaceSymbolProvider: register('workspaceSymbols'),
      registerRenameProvider: register('rename'),
    },
    debug: {
      registerDebugConfigurationProvider: (type, provider) => {
        log.debug.configuration = { type, provider };
        return { dispose() {} };
      },
      registerDebugAdapterDescriptorFactory: (type, factory) => {
        log.debug.adapter = { type, factory };
        return { dispose() {} };
      },
      startDebugging: (folder, config) => {
        log.started.push(config);
        return Promise.resolve(true);
      },
    },
    DebugAdapterExecutable: class {
      constructor(command, args) {
        this.command = command;
        this.args = args;
      }
    },
    Position: class {
      constructor(line, character) {
        this.line = line;
        this.character = character;
      }
    },
    Location: class {
      constructor(uri, range) {
        this.uri = uri;
        this.range = range;
      }
    },
    Hover: class {
      constructor(contents, range) {
        this.contents = contents;
        this.range = range;
      }
    },
    MarkdownString: class {
      constructor(value) {
        this.value = value;
      }
    },
    CompletionItem: class {
      constructor(label, kind) {
        this.label = label;
        this.kind = kind;
      }
    },
    SignatureHelp: class {},
    SignatureInformation: class {
      constructor(label, documentation) {
        this.label = label;
        this.documentation = documentation;
      }
    },
    ParameterInformation: class {
      constructor(label) {
        this.label = label;
      }
    },
    DocumentSymbol: class {
      constructor(name, detail, kind, range, selectionRange) {
        Object.assign(this, { name, detail, kind, range, selectionRange, children: [] });
      }
    },
    SymbolInformation: class {
      constructor(name, kind, containerName, location) {
        Object.assign(this, { name, kind, containerName, location });
      }
    },
    WorkspaceEdit: class {
      constructor() {
        this.edits = [];
      }
      replace(uri, range, newText) {
        this.edits.push({ uri: uri.toString(), range, newText });
      }
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
    DiagnosticSeverity: { Error: 0, Warning: 1, Information: 2, Hint: 3 },
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


// ---- the language features ----

const PROGRAM = [
  'package main',
  '',
  'type Point = record',
  '    x: int',
  '    y: int',
  'end',
  '',
  '// Adds two numbers.',
  'func add(a: int, b: int) -> int',
  '    return a + b',
  'end',
  '',
  'func main()',
  '    let p = Point{x: 1, y: 2}',
  '    let n = add(p.x, 4)',
  '    p.',
  '    print("{n}")',
  'end',
  '',
].join('\n');

/** Starts the extension against the real server with `PROGRAM` open. */
async function withProgram(fn) {
  const { vscode, extension } = load({ 'server.path': exe });
  await extension.activate(context());
  const d = doc('file:///features.zila', PROGRAM, 1);
  vscode.workspace.textDocuments.push(d);
  vscode.log.handlers.open(d);
  try {
    await fn(vscode.log, d, vscode);
  } finally {
    await extension.deactivate();
  }
}

const at = (line, character) => ({ line, character });

test('completion after a dot offers the fields and methods, and is triggered by the dot', { skip }, async () => {
  await withProgram(async (log, d) => {
    assert.deepStrictEqual(log.triggers.completion, ['.']);
    // line 15 is `    p.`: the cursor follows the dot
    const items = await log.providers.completion.provideCompletionItems(d, at(15, 6));
    const labels = items.map((i) => i.label).sort();
    assert.deepStrictEqual(labels, ['x', 'y']);
    assert.ok(items.every((i) => i.detail === 'int'));
    assert.strictEqual(items[0].kind, 4, 'a field');
  });
});

test('hover, signature help, definition, references and symbols answer from the server', { skip }, async () => {
  await withProgram(async (log, d) => {
    // hover on `add` in `add(p.x, 4)` (line 14)
    const h = await log.providers.hover.provideHover(d, at(14, 13));
    assert.ok(h.contents.value.includes('func add(a: int, b: int) -> int'), h.contents.value);
    assert.ok(h.contents.value.includes('Adds two numbers.'));
    // signature help after the comma of `add(p.x,`
    assert.deepStrictEqual(log.triggers.signature, ['(', ',']);
    const sig = await log.providers.signature.provideSignatureHelp(d, at(14, 22));
    assert.strictEqual(sig.activeParameter, 1);
    assert.deepStrictEqual(sig.signatures[0].parameters.map((p) => p.label), ['a: int', 'b: int']);
    // the definition of `add`: its declaration on line 8
    const def = await log.providers.definition.provideDefinition(d, at(14, 13));
    assert.strictEqual(def.range.start.line, 8);
    // its references, with the declaration
    const refs = await log.providers.references.provideReferences(d, at(14, 13), { includeDeclaration: true });
    assert.deepStrictEqual(refs.map((r) => r.range.start.line).sort((a, b) => a - b), [8, 14]);
    // the symbols of the document and of the workspace
    const symbols = await log.providers.symbols.provideDocumentSymbols(d);
    assert.deepStrictEqual(symbols.map((s) => s.name), ['Point', 'add', 'main']);
    assert.deepStrictEqual(symbols[0].children.map((c) => c.name), ['x', 'y']);
    const found = await log.providers.workspaceSymbols.provideWorkspaceSymbols('ad');
    assert.deepStrictEqual(found.map((s) => s.name), ['add']);
  });
});

test('rename returns the edits, and says why when it cannot', { skip }, async () => {
  await withProgram(async (log, d) => {
    const edit = await log.providers.rename.provideRenameEdits(d, at(14, 13), 'plus');
    assert.deepStrictEqual(edit.edits.map((e) => e.range.start.line).sort((a, b) => a - b), [8, 14]);
    assert.ok(edit.edits.every((e) => e.newText === 'plus'));
    await assert.rejects(() => log.providers.rename.provideRenameEdits(d, at(14, 13), 'main'), /already declared/);
    await assert.rejects(() => log.providers.rename.provideRenameEdits(d, at(14, 13), 'end'), /not a valid name/);
  });
});

test('an error of the checker is shown with its severity and the range of the expression', { skip }, async () => {
  const { vscode, extension } = load({ 'server.path': exe });
  await extension.activate(context());
  const log = vscode.log;
  // a text that parses but does not check: an error with a range
  log.handlers.open(doc('file:///e.zila', 'package main\n\nfunc main()\n    let x: int = "text"\nend\n', 1));
  await until(() => (log.diagnostics.get('file:///e.zila') || []).length === 1, 'the type error');
  const item = log.diagnostics.get('file:///e.zila')[0];
  assert.strictEqual(item.severity, 0);
  assert.deepStrictEqual([item.range.start.line, item.range.start.character, item.range.end.character], [3, 17, 23]);
  await extension.deactivate();
});

// ---- the debugger ----

test('the zila debugger runs the same executable with --dap and starts from the file in the editor', async () => {
  const { vscode, extension } = load({ 'server.path': '/tools/zila' });
  await extension.activate(context());
  const { adapter, configuration } = vscode.log.debug;
  assert.strictEqual(adapter.type, 'zila');
  assert.strictEqual(configuration.type, 'zila');
  const executable = adapter.factory.createDebugAdapterDescriptor();
  assert.deepStrictEqual([executable.command, executable.args], ['/tools/zila', ['--dap']]);

  // nothing configured: the file that is open
  assert.strictEqual(configuration.provider.resolveDebugConfiguration(undefined, {}), undefined, 'no file open');
  vscode.window.activeTextEditor = { document: { languageId: 'zila', fileName: '/work/hello.zila' } };
  assert.deepStrictEqual(configuration.provider.resolveDebugConfiguration(undefined, {}), { type: 'zila', request: 'launch', name: 'Debug current file', program: '/work/hello.zila', stopOnEntry: false });
  // a configuration without a program gets the open file; one that has it is left alone
  assert.strictEqual(configuration.provider.resolveDebugConfiguration(undefined, { type: 'zila', request: 'launch', name: 'x' }).program, '/work/hello.zila');
  assert.strictEqual(configuration.provider.resolveDebugConfiguration(undefined, { type: 'zila', request: 'launch', name: 'x', program: '/a.zila' }).program, '/a.zila');
  // the command starts a session for the file
  await vscode.log.commands.get('zila.debugFile')();
  assert.strictEqual(vscode.log.started[0].program, '/work/hello.zila');
  await extension.deactivate();
});
