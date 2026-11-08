// A small Language Server Protocol client for `zila --lsp`.
//
// It has no dependencies and does not import `vscode`, so it can be tested with plain Node
// (see test/client.test.js). It speaks the part of the protocol the Zila Lang server offers:
// initialize, full-text document sync, publishDiagnostics, formatting, shutdown and exit.

'use strict';

const childProcess = require('child_process');

const RESTARTS = 3;
const RESTART_WINDOW_MS = 60000;
const FORMAT_TIMEOUT_MS = 5000;

class Client {
  /**
   * options:
   *   command      executable to start (default "zila")
   *   args         its arguments (default ["--lsp"])
   *   spawn        function like child_process.spawn (for tests)
   *   debounce     ms to wait before sending a burst of changes (default 150)
   *   onDiagnostics(uri, diagnostics, version)
   *   onError(error)   the server could not be started
   *   onStopped()      the server kept exiting and will not be restarted
   *   log(line)        protocol trace
   */
  constructor(options) {
    this.command = options.command || 'zila';
    this.args = options.args || ['--lsp'];
    this.spawn = options.spawn || childProcess.spawn;
    this.debounce = options.debounce === undefined ? 150 : options.debounce;
    this.onDiagnostics = options.onDiagnostics || (() => {});
    this.onError = options.onError || (() => {});
    this.onStopped = options.onStopped || (() => {});
    this.log = options.log || (() => {});
    this.docs = new Map(); // uri -> { text, version, sent }
    this.timers = new Map();
    this.pending = new Map(); // request id -> { resolve, timer }
    this.nextId = 1;
    this.proc = null;
    this.buffer = Buffer.alloc(0);
    this.ready = null;
    this.stopping = false;
    this.exits = [];
  }

  // ---- process ----

  start() {
    this.stopping = false;
    this.initialized = false;
    this.buffer = Buffer.alloc(0);
    let proc;
    try {
      proc = this.spawn(this.command, this.args, { stdio: ['pipe', 'pipe', 'pipe'], shell: false });
    } catch (e) {
      this.onError(e);
      return Promise.resolve(false);
    }
    this.proc = proc;
    let settled = false;
    this.ready = new Promise((resolve) => {
      proc.on('error', (e) => {
        if (this.proc !== proc) return;
        this.proc = null;
        if (!settled) {
          settled = true;
          resolve(false);
        }
        this.onError(e);
      });
      proc.on('exit', (code) => {
        if (this.proc !== proc) return;
        this.proc = null;
        this.initialized = false;
        this.failPending();
        if (!settled) {
          settled = true;
          resolve(false);
        }
        if (!this.stopping) this.crashed(code);
      });
      proc.stdout.on('data', (chunk) => this.read(chunk));
      if (proc.stderr) proc.stderr.on('data', (chunk) => this.log('[stderr] ' + chunk.toString().trimEnd()));
      this.request('initialize', { processId: process.pid, rootUri: null, capabilities: {} }).then((r) => {
        if (r === undefined) return;
        this.initialized = true;
        this.notify('initialized', {});
        // documents opened before the server was ready, or before a restart
        for (const [uri, d] of this.docs) this.sendOpen(uri, d);
        if (!settled) {
          settled = true;
          resolve(true);
        }
      });
    });
    return this.ready;
  }

  crashed(code) {
    const now = Date.now();
    this.exits = this.exits.filter((t) => now - t < RESTART_WINDOW_MS);
    this.exits.push(now);
    this.log('[client] the server exited with code ' + code);
    if (this.exits.length > RESTARTS) {
      this.onStopped();
      return;
    }
    this.start();
  }

  failPending() {
    for (const [, p] of this.pending) {
      clearTimeout(p.timer);
      p.resolve(undefined);
    }
    this.pending.clear();
  }

  /** Sends shutdown and exit, then kills the process if it does not leave in 2 seconds. */
  async stop() {
    this.stopping = true;
    for (const t of this.timers.values()) clearTimeout(t);
    this.timers.clear();
    const proc = this.proc;
    if (!proc) return;
    await this.request('shutdown', null, 2000);
    this.notify('exit');
    await new Promise((resolve) => {
      const kill = setTimeout(() => {
        try {
          proc.kill();
        } catch (e) {
          // already gone
        }
        resolve();
      }, 2000);
      proc.on('exit', () => {
        clearTimeout(kill);
        resolve();
      });
      try {
        proc.stdin.end();
      } catch (e) {
        // already closed
      }
    });
    this.proc = null;
  }

  // ---- framing ----

  send(message) {
    if (!this.proc || !this.proc.stdin.writable) return false;
    const body = JSON.stringify(message);
    this.log('--> ' + body);
    this.proc.stdin.write('Content-Length: ' + Buffer.byteLength(body) + '\r\n\r\n' + body);
    return true;
  }

  read(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);
    for (;;) {
      const end = this.buffer.indexOf('\r\n\r\n');
      if (end < 0) return;
      const header = this.buffer.slice(0, end).toString();
      const m = /content-length:\s*(\d+)/i.exec(header);
      if (!m) {
        // not a frame: drop the header and go on
        this.buffer = this.buffer.slice(end + 4);
        continue;
      }
      const length = parseInt(m[1], 10);
      if (this.buffer.length < end + 4 + length) return;
      const body = this.buffer.slice(end + 4, end + 4 + length).toString();
      this.buffer = this.buffer.slice(end + 4 + length);
      let message;
      try {
        message = JSON.parse(body);
      } catch (e) {
        this.log('[client] bad JSON from the server');
        continue;
      }
      this.log('<-- ' + body);
      this.handle(message);
    }
  }

  handle(message) {
    if (message.id !== undefined && message.method === undefined) {
      const p = this.pending.get(message.id);
      if (p) {
        this.pending.delete(message.id);
        clearTimeout(p.timer);
        p.resolve(message.error ? null : message.result === undefined ? null : message.result);
      }
      return;
    }
    if (message.method === 'textDocument/publishDiagnostics') {
      const params = message.params || {};
      const doc = this.docs.get(params.uri);
      // a document that was closed meanwhile has nothing to show, except the clearing message
      if (!doc && params.diagnostics && params.diagnostics.length > 0) return;
      this.onDiagnostics(params.uri, params.diagnostics || [], doc ? doc.version : undefined);
    }
  }

  request(method, params, timeoutMs) {
    return new Promise((resolve) => {
      const id = this.nextId++;
      const timer = setTimeout(() => {
        this.pending.delete(id);
        resolve(undefined);
      }, timeoutMs || FORMAT_TIMEOUT_MS);
      this.pending.set(id, { resolve, timer });
      if (!this.send({ jsonrpc: '2.0', id, method, params })) {
        clearTimeout(timer);
        this.pending.delete(id);
        resolve(undefined);
      }
    });
  }

  notify(method, params) {
    const m = { jsonrpc: '2.0', method };
    if (params !== undefined) m.params = params;
    this.send(m);
  }

  // ---- documents ----

  sendOpen(uri, d) {
    d.sent = d.version;
    this.notify('textDocument/didOpen', { textDocument: { uri, languageId: 'zila', version: d.version, text: d.text } });
  }

  open(uri, text, version) {
    const d = { text, version: version || 1, sent: null };
    this.docs.set(uri, d);
    if (this.proc && this.isReady()) this.sendOpen(uri, d);
  }

  isReady() {
    return this.proc !== null && this.initialized === true;
  }

  /** Records new text and sends it after the debounce (or at once when `now`). */
  change(uri, text, version, now) {
    const d = this.docs.get(uri);
    if (!d) return this.open(uri, text, version);
    d.text = text;
    d.version = version || d.version + 1;
    clearTimeout(this.timers.get(uri));
    if (now || this.debounce === 0) return this.flush(uri);
    this.timers.set(
      uri,
      setTimeout(() => this.flush(uri), this.debounce)
    );
  }

  flush(uri) {
    clearTimeout(this.timers.get(uri));
    this.timers.delete(uri);
    const d = this.docs.get(uri);
    if (!d || d.sent === d.version || !this.proc) return;
    d.sent = d.version;
    this.notify('textDocument/didChange', {
      textDocument: { uri, version: d.version },
      contentChanges: [{ text: d.text }],
    });
  }

  close(uri) {
    clearTimeout(this.timers.get(uri));
    this.timers.delete(uri);
    if (!this.docs.has(uri)) return;
    this.docs.delete(uri);
    this.notify('textDocument/didClose', { textDocument: { uri } });
  }

  /** The formatting edits of a document, or null (nothing to change, or no answer in time). */
  async format(uri) {
    this.flush(uri);
    if (!this.docs.has(uri)) return null;
    const edits = await this.request('textDocument/formatting', {
      textDocument: { uri },
      options: { tabSize: 4, insertSpaces: true },
    });
    return Array.isArray(edits) && edits.length > 0 ? edits : null;
  }
}

module.exports = { Client, RESTARTS, RESTART_WINDOW_MS, FORMAT_TIMEOUT_MS };
