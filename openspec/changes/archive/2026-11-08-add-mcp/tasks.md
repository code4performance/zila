## 1. Server core

- [x] 1.1 Register `mcp` in the library tables of `src/codegen_x64.rs` (rejected in the bare-metal build) and write the JSON-RPC core and `Server`: `handle`, lifecycle (`initialize`, `initialized`, `ping`, version negotiation, pre-initialize rejection), standard error codes, capabilities only for what is registered. Verify: `tests/mcp.rs` calls `handle` with the spec's handshake, before-initialize and unknown-method scenarios; the kernel build rejects `use mcp` naming the library.
- [x] 1.2 Tools over MCP: list with schemas from `ai.Tool`, call with argument errors as `-32602`, handler errors as `isError`, unknown tool, concurrency limit (no per-call time limit). Verify: `tests/mcp.rs` list/call/failure scenarios; the same `ai.Tool` value is served by the server and called directly (running it in `client.run` is covered by `tests/ai.rs`).
- [x] 1.3 Resources (static and templates), prompts, cursors and page size, error `-32002`. Verify: `tests/mcp.rs` read, template match, unknown URI, and a 250-item list paged by cursor.

## 2. Transports

- [x] 2.1 stdio: line framing, nothing else on standard output, parse-error handling (one request at a time; cancellation is accepted and ignored). Verify: `tests/mcp.rs` compiles a server program and drives it through pipes (three response lines for handshake, noise and a call).
- [x] 2.2 Streamable HTTP on `web`: `POST`/`GET`/`DELETE`, sessions, protocol version header, `Origin` allow list, bearer token with `crypto.equal`, loopback default and refusal to bind elsewhere without a token, size limit. Verify: `tests/mcp.rs` session, unknown session `404`, bad origin `403`, missing token `401`, notification `202`, oversize message error, a wrong protocol version header.

## 3. Client

- [x] 3.1 `mcp.connect` with `tools`, `call`, `resources`, `read`, `prompts`, `prompt`, JSON and event-stream replies, JSON-RPC errors as errors, and `ai_tools()`. Verify: `tests/mcp.rs` client against the library's own server over loopback, remote tools as `ai.Tool` values (the event-stream reply parser is written but not exercised: the library's own server answers with JSON).
- [x] 3.2 Interoperability by hand with the official MCP Inspector 2.13.0, over stdio (lists, tool calls, an error result, an unknown tool, a resource, a prompt) and Streamable HTTP (with the token; without it the Inspector takes the `401` as an authorization challenge); the results are in `tests/data/mcp-interop.md`. A host (Claude Code) was not run: it changes the host's configuration, so it is left to the owner and recorded there as not run.

## 4. Example and documentation

- [x] 4.1 `examples/mcp_server.br`: stdio, narrow parameterized tools, read-only `run_select`, a schema resource, a prompt, host configuration in the header, over `db.Conn` (PostgreSQL). Verify: `tests/mcp.rs` builds it and `cargo test --test fmt` checks its formatting (it needs a database to run, so the suite does not run it).
- [x] 4.2 MCP section in `docs/ai.html` (server, client, transports, safety defaults, least-privilege advice, the example), updates to `docs/stdlib.html`, `docs/native.html`, navigation and `README.md`. Verify: `cargo test --test docs` passes.
- [x] 4.3 Windows and Linux parity for the `mcp` fixtures in `tests/linux.rs`. Verify: `cargo test --test linux` passes (leave `tests/async.rs` out of routine runs).
- [x] 4.4 `openspec validate add-mcp --strict` passes and full `cargo test` (without `tests/async.rs`) is green.
