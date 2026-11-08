# MCP interoperability

What was run by hand against the `mcp` library, and what is still to be run against software written
by someone else. The automated tests (`tests/mcp.rs`) cover the protocol with the library's own client;
this file is the record of the checks that need other implementations.

## Done: a full stdio session with the real example (2026-11-08)

`examples/mcp_server.br` built to an executable (with its PostgreSQL URL pointed at a local server) and
driven by writing JSON-RPC lines to its standard input, as a host would. Nine requests, nine answers,
in order, one JSON object per line, nothing else on standard output:

| request | answer |
|---|---|
| `initialize` (2025-06-18) | the same protocol version, capabilities for tools, resources and prompts, `serverInfo` customers 1.0 |
| `notifications/initialized` | no answer (a notification) |
| `tools/list` | `find_customer` and `run_select`, each with an `inputSchema` and the description of its parameter |
| `tools/call run_select` with `select 1 as one, 'zila' as name` | text `one \| name` / `1 \| zila`, `isError` false |
| `tools/call run_select` with `drop table x` | `isError` true, "only a single SELECT statement is accepted" |
| `resources/list`, `resources/read` | the `db://schema/customers` resource and its text |
| `prompts/list`, `prompts/get customer_report` | the prompt with its argument, and the message with the name filled in |
| `ping` | an empty result |

## Done: the official MCP Inspector, version 2.13.0 (2026-11-08)

Run with `npx -y @modelcontextprotocol/inspector --cli ...` against the executable built from
`examples/mcp_server.br` (stdio) and against a small program with `serve_http` and a bearer token
(Streamable HTTP, listening on 127.0.0.1:8765).

| check | result |
|---|---|
| stdio, `tools/list` | both tools with their input schemas and parameter descriptions |
| stdio, `resources/list`, `resources/read` | the `db://schema/customers` resource and its text |
| stdio, `prompts/list`, `prompts/get customer_report name=Ana` | the prompt, its argument, and the message with the name filled in |
| stdio, `tools/call run_select` with a `select` | `one \| name` / `1 \| zila`, `isError` false |
| stdio, `tools/call run_select` with `drop table x` | the Inspector reports `isError:true` with the server's message |
| stdio, call of a tool that does not exist | the Inspector reports `tool_not_found` |
| Streamable HTTP with `Authorization: Bearer secret`, `tools/list` and `tools/call echo` | the tool is listed, and the call returns `echo: Zé` (non-ASCII text intact) |
| Streamable HTTP without a token, and with a wrong one | the server answers `401` with `WWW-Authenticate: Bearer`; the Inspector takes it as an authorization challenge and starts its OAuth flow (it needs a terminal), so nothing is served |
| the server's own start-up line | `listening on 127.0.0.1:8765`: the default address is the loopback |

No difference was found between what the Inspector expects and what the library sends.

## Not run: a host (Claude Code or another)

In the host's MCP configuration (Claude Code: `claude mcp add customers -- C:/path/to/mcp_server.exe`;
Claude Desktop: `{ "mcpServers": { "customers": { "command": "C:/path/to/mcp_server.exe" } } }`), start
the host, check that the server shows as connected with its two tools, and ask for a customer by name.
This changes the host's configuration, so it was left to the owner; the Inspector run above covers the
protocol from a client written by someone else.
