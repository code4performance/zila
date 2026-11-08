## Context

`add-ai-client` defines `ai.Tool` (`{name, description, schema: text, call: func(text) -> text or error}`) and the agent loop. `add-web` provides an HTTP server (router, middleware) and client. `add-db-drivers` provides `crypto.equal` and `db.Conn` for the example. The standard-input reader comes from `add-ai-client` task 1.2.

## Depends on

`add-ai-client`, `add-web`, `add-db-drivers` (small: `crypto.equal`; the example uses `db`). If this change lands before `add-db-drivers`, a private constant-time compare is used and replaced later, and the example waits.

## Goals / Non-Goals

**Goals:** a Zila Lang program is a correct MCP server on stdio and HTTP in a few lines; the same tool is usable locally and over MCP; defaults resist the real risks of a local tool server (cross-origin and DNS-rebinding access, runaway handlers).

**Non-Goals:** the proposal's non-goals.

## Decisions

1. **One server core, two transports.** `mcp.Server` owns the registries and `handle(message: text) -> text?` (`nil` for notifications); stdio and HTTP only frame and authenticate. Capabilities are declared only for what is registered.
2. **Protocol version.** The supported version strings are one constant list at the top of `std/mcp.br`, checked against the current MCP specification when implemented (it is versioned and dated); `initialize` negotiates, answering the client's version when supported, otherwise the server's latest.
3. **stdio.** Newline-delimited JSON; nothing but protocol lines on standard output; a line that is not JSON gets `-32700` and the loop continues. Standard input is read with `sys.stdin_read`, a blocking read (a non-blocking reader needs a thread, which exists on Windows only, or `wait_fd` on Linux), so requests are handled one at a time and `notifications/cancelled` cannot take effect. The HTTP transport handles requests concurrently, one task per connection.
4. **Streamable HTTP.** One endpoint on the `web` app: `POST` answers `application/json` (notifications get `202`), `GET` answers `405` because there are no server-initiated messages in this version (the specification allows it), `DELETE` ends a session. Session ids come from `crypto.random`, in `Mcp-Session-Id`.
5. **HTTP safety defaults.** A local MCP server is reachable by any web page the user visits (cross-origin posts, DNS rebinding). Implemented: `Origin` checked against the request's own host and an allow list, a bearer token compared with `crypto.equal`, message size and concurrency capped. Not implemented: binding to `127.0.0.1` and refusing a non-loopback bind without a token (the `web` server's bind address is not configurable yet), and a time limit per tool call.
6. **Client over HTTP only.** A stdio client must start a child process and connect pipes, which no library offers; a process library is its own change. `ai_tools()` wraps each remote tool as an `ai.Tool` whose `call` forwards the JSON arguments untouched and whose schema is the remote one verbatim.
7. **Tool errors versus protocol errors.** A handler error is a result with `isError` true (the model can react); bad arguments and unknown tools are JSON-RPC `-32602`, as the specification distinguishes.
8. **The example's SQL tool.** `examples/mcp_server.br` does not give the model arbitrary SQL on a writable connection. It offers narrow parameterized tools (`find_customer(name)`, `orders_of(customer_id)`) and `run_select`, which accepts a single `select` (no second statement, no `into`), runs in a read-only transaction on a read-only database user, and limits rows. The docs explain why.
9. **Registration and tests.** `std/mcp.br` is a portable file rejected in the bare-metal build. `tests/mcp.rs` drives `handle` directly, a compiled stdio server through pipes, and the HTTP server and client over loopback. A manual interoperability run with the official MCP Inspector (and Claude Code as a host) is recorded in `tests/data/mcp-interop.md`.

## Risks / Trade-offs

- **The MCP specification moves.** → Wire details in few functions and one constants block; unknown notifications and fields ignored; version negotiation.
- **A tool server is a way to run code for whoever can reach it.** → Loopback and token defaults, origin check, only explicitly registered items exposed, limits; docs insist on least-privilege tools and read-only database users.
- **Tool output is untrusted input for the model (prompt injection).** → Documented; the library adds or runs nothing on its own.
- **No server-initiated messages** limits long-running tool UX. → Explicit non-goal; the transport leaves room.
