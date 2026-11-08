## Why

The Model Context Protocol is how AI hosts (Claude Desktop, Claude Code, IDEs) call tools, read resources and use prompts from other programs. A Zila Lang program that can be an MCP server can expose a database, a knowledge base or any function to those hosts, and one that can be an MCP client can use remote tools in its own agent. With `add-ai-client`'s `ai.Tool`, one tool definition serves both a local agent and an MCP server.

## What Changes

- **`use mcp` server**: JSON-RPC 2.0 and the MCP lifecycle; tools (from `ai.Tool`), resources and templates, prompts; transports **stdio** and **Streamable HTTP** (on `use web`) with sessions, `Origin` checks, a bearer token, loopback default and size, concurrency and time limits.
- **`use mcp` client** over Streamable HTTP: `tools`, `call`, `resources`, `read`, `prompts`, `prompt`, JSON or event-stream replies, and `ai_tools()` to use remote tools in `ai`'s agent loop.
- **Example** `examples/mcp_server.br`: narrow parameterized database tools, a read-only `run_select` tool and a schema resource over PostgreSQL or SQL Server, with the host configuration in the file header.
- **Documentation**: an MCP section in `docs/ai.html`, stdlib, navigation.

Non-goals: an MCP stdio client (starting child processes needs a process library that does not exist; a follow-up); OAuth authorization beyond a static bearer token; server-initiated messages (sampling, roots, progress, subscriptions, log notifications) in this version; batches.

## Capabilities

### New Capabilities

- `mcp-library`: MCP server (stdio and HTTP) and HTTP client.

### Modified Capabilities

None.

## Impact

New `std/mcp.br`; `src/codegen_x64.rs` (library table, kernel rejection); `examples/mcp_server.br`; `tests/mcp.rs`, `tests/linux.rs`; `tests/data/mcp-interop.md`; `docs/ai.html`, `docs/stdlib.html`, `docs/native.html`, navigation, `README.md`. Depends on `add-ai-client` (`ai.Tool`, `client.run`), `add-web` (HTTP server and client), `add-db-drivers` (only for the example and its real-server test, which use `db.Conn`), and `crypto.equal` from `add-db-drivers` (constant-time token comparison).
