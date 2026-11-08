// Tests of the language client against the real `zila --lsp` (no VS Code needed).
// Run with: node --test editors/vscode/test
// The binary is found in $ZILA_BIN, or target/debug, or target/release.

'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { Client } = require('../client');

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
const GOOD = 'package main\n\nfunc main()\nend\n';
const BAD = 'package main\n\nfunc main()\n    let = 3\nend\n';
const UNFORMATTED = 'package main\n\nfunc main()\nprintln(1,2)\nend\n';
const FORMATTED = 'package main\n\nfunc main()\n    println(1, 2)\nend\n';

/** A client whose diagnostics can be awaited one by one. */
function make(options) {
  const queue = [];
  const waiting = [];
  const client = new Client(
    Object.assign(
      {
        command: exe,
        debounce: 0,
        onDiagnostics: (uri, list, version) => {
          const item = { uri, list, version };
          if (waiting.length) waiting.shift()(item);
          else queue.push(item);
        },
      },
      options
    )
  );
  client.next = () => new Promise((resolve) => (queue.length ? resolve(queue.shift()) : waiting.push(resolve)));
  return client;
}

const skip = exe ? false : 'zila is not built (cargo build)';

test('the handshake completes and the server stops cleanly', { skip }, async () => {
  const c = make();
  assert.strictEqual(await c.start(), true);
  await c.stop();
});

test('a good file has no diagnostics, a bad one has one, and fixing it clears it', { skip }, async () => {
  const c = make();
  await c.start();
  c.open('file:///a.zila', GOOD, 1);
  assert.deepStrictEqual((await c.next()).list, []);
  c.change('file:///a.zila', BAD, 2);
  const bad = await c.next();
  assert.strictEqual(bad.list.length, 1);
  assert.strictEqual(bad.list[0].range.start.line, 3);
  assert.strictEqual(bad.list[0].severity, 1);
  assert.strictEqual(bad.list[0].source, 'zila');
  assert.strictEqual(bad.version, 2);
  c.change('file:///a.zila', GOOD, 3);
  assert.deepStrictEqual((await c.next()).list, []);
  c.close('file:///a.zila');
  assert.deepStrictEqual((await c.next()).list, []);
  await c.stop();
});

test('documents opened before the server is ready are sent once it is', { skip }, async () => {
  const c = make();
  const started = c.start();
  c.open('file:///early.zila', BAD, 1);
  await started;
  assert.strictEqual((await c.next()).list.length, 1);
  await c.stop();
});

test('typing bursts are sent once after the debounce', { skip }, async () => {
  const c = make({ debounce: 60 });
  await c.start();
  c.open('file:///burst.zila', GOOD, 1);
  await c.next();
  c.change('file:///burst.zila', BAD, 2);
  c.change('file:///burst.zila', GOOD, 3);
  c.change('file:///burst.zila', BAD, 4);
  const d = await c.next();
  assert.strictEqual(d.version, 4);
  assert.strictEqual(d.list.length, 1);
  await c.stop();
});

test('formatting returns one edit for an unformatted file and none for a formatted one', { skip }, async () => {
  const c = make();
  await c.start();
  c.open('file:///f.zila', UNFORMATTED, 1);
  await c.next();
  const edits = await c.format('file:///f.zila');
  assert.strictEqual(edits.length, 1);
  assert.strictEqual(edits[0].newText, FORMATTED);
  assert.deepStrictEqual(edits[0].range.start, { line: 0, character: 0 });
  c.change('file:///f.zila', FORMATTED, 2, true);
  await c.next();
  assert.strictEqual(await c.format('file:///f.zila'), null);
  c.change('file:///f.zila', BAD, 3, true);
  await c.next();
  assert.strictEqual(await c.format('file:///f.zila'), null);
  await c.stop();
});

test('formatting flushes a pending change first', { skip }, async () => {
  const c = make({ debounce: 5000 });
  await c.start();
  c.open('file:///p.zila', GOOD, 1);
  await c.next();
  c.change('file:///p.zila', UNFORMATTED, 2);
  const edits = await c.format('file:///p.zila');
  assert.strictEqual(edits[0].newText, FORMATTED);
  await c.stop();
});

test('a missing executable is reported once and start resolves false', async () => {
  const errors = [];
  const c = new Client({ command: path.join(os.tmpdir(), 'no such dir', 'zila'), onError: (e) => errors.push(e) });
  assert.strictEqual(await c.start(), false);
  assert.strictEqual(errors.length, 1);
  assert.strictEqual(errors[0].code, 'ENOENT');
});

// A fake server for the crash tests: answers initialize, then exits when it gets `didOpen`.
function fakeServer(dir, exitOnOpen) {
  const file = path.join(dir, 'fake-server.js');
  const log = path.join(dir, 'fake-starts.txt');
  fs.writeFileSync(
    file,
    `
const fs = require('fs');
fs.appendFileSync(${JSON.stringify(log)}, 'start\\n');
let buf = '';
process.stdin.on('data', (d) => {
  buf += d;
  for (;;) {
    const i = buf.indexOf('\\r\\n\\r\\n');
    if (i < 0) return;
    const n = parseInt(/content-length:\\s*(\\d+)/i.exec(buf.slice(0, i))[1], 10);
    if (buf.length < i + 4 + n) return;
    const msg = JSON.parse(buf.slice(i + 4, i + 4 + n));
    buf = buf.slice(i + 4 + n);
    if (msg.method === 'initialize') {
      const body = JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: { capabilities: {} } });
      process.stdout.write('Content-Length: ' + Buffer.byteLength(body) + '\\r\\n\\r\\n' + body);
    }
    if (msg.method === 'textDocument/didOpen' && ${exitOnOpen}) process.exit(3);
    if (msg.method === 'exit') process.exit(0);
  }
});
`
  );
  return { file, log };
}

test('a crashing server is restarted three times with the open documents, then reported', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'zila-fake-'));
  const { file, log } = fakeServer(dir, true);
  let stopped = false;
  const c = new Client({ command: process.execPath, args: [file], onStopped: () => (stopped = true) });
  await c.start();
  c.open('file:///x.zila', GOOD, 1);
  for (let i = 0; i < 100 && !stopped; i++) await new Promise((r) => setTimeout(r, 50));
  assert.strictEqual(stopped, true);
  // the first start plus three restarts
  assert.strictEqual(fs.readFileSync(log, 'utf8').trim().split('\n').length, 4);
  fs.rmSync(dir, { recursive: true, force: true });
});

test('split and merged reads are framed correctly', async () => {
  const seen = [];
  const c = new Client({ onDiagnostics: (uri, list) => seen.push([uri, list.length]) });
  const frame = (uri, n) => {
    const body = JSON.stringify({ jsonrpc: '2.0', method: 'textDocument/publishDiagnostics', params: { uri, diagnostics: new Array(n).fill({}) } });
    return Buffer.from('Content-Length: ' + Buffer.byteLength(body) + '\r\n\r\n' + body);
  };
  const a = frame('file:///1', 0);
  const b = frame('file:///2', 0);
  // two frames in one chunk, and one frame cut in the middle of the header and of the body
  c.read(Buffer.concat([a, b]));
  const third = frame('file:///3', 0);
  c.read(third.slice(0, 7));
  c.read(third.slice(7, third.length - 3));
  c.read(third.slice(third.length - 3));
  assert.deepStrictEqual(
    seen.map((s) => s[0]),
    ['file:///1', 'file:///2', 'file:///3']
  );
});
