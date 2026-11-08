## 1. Server core

- [ ] 1.1 Register `mcp` in the library tables of `src/codegen_x64.rs` (rejected in the bare-metal build) and write the JSON-RPC core and `Server`: `handle`, lifecycle (`initialize`, `initialized`, `ping`, version negotiation, pre-initialize rejection), standard error codes, capabilities only for what is registered. Verify: `tests/mcp.rs` calls `handle` with the spec's handshake, before-initialize and unknown-method scenarios; the kernel build rejects `use mcp` naming the library.
- [ ] 1.2 Tools over MCP: list with schemas from `ai.Tool`, call with argument errors as `-32602`, handler errors as `isError`, unknown tool, timeout, concurrency limit. Verify: `tests/mcp.rs` list/call/failure scenarios; the same `ai.Tool` value runs in `client.run` against `add-ai-client`'s fake model server.
- [ ] 1.3 Resources (static and templates), prompts, cursors and page size, error `-32002`. Verify: `tests/mcp.rs` read, template match, unknown URI, and a 250-item list paged by cursor.

## 2. Transports

- [ ] 2.1 stdio: line framing, nothing else on standard output, parse-error handling, concurrent requests, cancellation. Verify: `tests/mcp.rs` compiles a server program and drives it through pipes (two response lines for handshake plus call, a noise line between requests, cancellation drops the answer).
- [ ] 2.2 Streamable HTTP on `web`: `POST`/`GET`/`DELETE`, sessions, protocol version header, `Origin` allow list, bearer token with `crypto.equal`, loopback default and refusal to bind elsewhere without a token, size limit. Verify: `tests/mcp.rs` session, unknown session `404`, bad origin `403`, missing token `401`, notification `202`, oversize message error, a non-loopback bind without token is refused.

## 3. Client

- [ ] 3.1 `mcp.connect` with `tools`, `call`, `resources`, `read`, `prompts`, `prompt`, JSON and event-stream replies, JSON-RPC errors as errors, and `ai_tools()`. Verify: `tests/mcp.rs` client against the library's own server over loopback, an event-stream reply with a notification before the response, and an agent run that uses a remote tool.
- [ ] 3.2 Interoperability by hand with the official MCP Inspector and with Claude Code as host, for stdio and HTTP; record in `tests/data/mcp-interop.md`. Verify: the document lists the tool list and call that worked in each transport; any failure becomes a fix or a spec change before this task is done.

## 4. Example and documentation

- [ ] 4.1 `examples/mcp_server.br`: stdio and HTTP, narrow parameterized tools, read-only `run_select`, a schema resource, host configuration in the header, over `db.Conn` (PostgreSQL or SQL Server). Verify: builds and passes `brasa fmt --check`; `tests/mcp.rs` drives it through pipes with a fake driver; the `run_select` test rejects `delete`, a second statement and a `select ... into`; with `BRASA_TEST_POSTGRES` set it also runs on the Docker server.
- [ ] 4.2 MCP section in `docs/ai.html` (server, client, transports, safety defaults, least-privilege advice, the example), updates to `docs/stdlib.html`, `docs/native.html`, navigation and `README.md`. Verify: `cargo test --test docs` passes.
- [ ] 4.3 Windows and Linux parity for the `mcp` fixtures in `tests/linux.rs`. Verify: `cargo test --test linux` passes (leave `tests/async.rs` out of routine runs).
- [ ] 4.4 `openspec validate add-mcp --strict` passes and full `cargo test` (without `tests/async.rs`) is green.
