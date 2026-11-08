## Purpose

Lets a Zila Lang program be an MCP server, exposing its functions, data and prompts to any MCP host, and be an MCP client whose remote tools an agent can use.

## ADDED Requirements

### Requirement: JSON-RPC and lifecycle
`use mcp` SHALL implement JSON-RPC 2.0 as MCP uses it: requests with ids, notifications without, batches not supported (answered with an error), and error objects with the standard codes (`-32700` parse error, `-32600` invalid request, `-32601` method not found, `-32602` invalid params, `-32603` internal error). A server SHALL answer `initialize` with the protocol version it supports (the version the client asked for when the server supports it, otherwise the server's latest), its `serverInfo` and its `capabilities` (only those it has: `tools`, `resources`, `prompts`), accept `notifications/initialized`, answer `ping`, and reject any other request sent before `initialize` with `-32600`. Unknown notifications SHALL be ignored.

#### Scenario: Handshake
- **WHEN** a client sends `initialize` and then `notifications/initialized` to a server that has one tool
- **THEN** the server's reply names a protocol version and lists the `tools` capability only

#### Scenario: Before initialize
- **WHEN** a client sends `tools/list` first
- **THEN** the server answers an error with code `-32600`

#### Scenario: Unknown method
- **WHEN** a client sends the method `nothing/here` after initializing
- **THEN** the server answers an error with code `-32601`

### Requirement: Tools
A server SHALL expose the `ai.Tool` values given to it (`server.tool(t)`) through `tools/list` (name, description, `inputSchema` derived from the input record) and `tools/call`. A call SHALL decode `arguments` into the tool's input type; arguments that do not match SHALL be answered with a JSON-RPC error `-32602` naming the field. A handler's `text` result SHALL be returned as a text content item with `isError` false, and a handler error SHALL be returned as a result with `isError` true and the message as text (a tool failure is not a protocol failure). A call to an unknown tool SHALL be answered with `-32602`. The same tool value SHALL be usable by `ai`'s agent loop and by a server without change.

#### Scenario: List and call
- **WHEN** a server has a `query_customers` tool with input `{ name: text }` and a client lists tools and then calls it with `{"name":"Ana"}`
- **THEN** the list contains the tool with an object schema having a required `name`, and the call returns the handler's text with `isError` false

#### Scenario: Handler failure
- **WHEN** a handler returns an error
- **THEN** the result has `isError` true and the message as content, and the connection stays open

#### Scenario: Same tool in an agent
- **WHEN** a tool is registered in an MCP server and also passed to `client.run` in another program
- **THEN** both call the same handler function with the same decoded input

### Requirement: Resources and prompts
A server SHALL expose resources (`server.resource(uri, name, mime_type, reader)`, and templates with `{param}` segments) through `resources/list`, `resources/templates/list` and `resources/read`, returning text content with its URI and MIME type; reading an unknown URI SHALL give error `-32002`. A server SHALL expose prompts (`server.prompt(name, description, args, builder)`, `args` a list of `mcp.PromptArg` with a name, a description and whether it is required) through `prompts/list` and `prompts/get`, where `builder` receives the arguments the host gave (a missing required one is an invalid-params error) and returns the list of messages. Both lists SHALL support the `cursor` parameter with a page size of at most 100.

#### Scenario: Read a resource
- **WHEN** a client reads `db://schema/customers`, registered with a reader that returns the table's DDL
- **THEN** the content item holds that text with the registered MIME type

#### Scenario: Unknown resource
- **WHEN** a client reads a URI nothing matches
- **THEN** the server answers error `-32002`

### Requirement: stdio transport
`server.serve_stdio()` SHALL read newline-delimited JSON-RPC messages from standard input and write one per line to standard output until input ends, and SHALL write nothing else to standard output (logs go to standard error). A message SHALL NOT contain a raw newline. A line that is not valid JSON SHALL get a `-32700` answer and the loop SHALL continue. Requests SHALL be handled one at a time (reading standard input blocks the program), so `notifications/cancelled` is accepted and ignored.

#### Scenario: Round trip over pipes
- **WHEN** a test writes `initialize`, `initialized` and a `tools/call` to the program's standard input and reads standard output
- **THEN** it receives exactly two response lines, the second with the tool's result

#### Scenario: Noise on input
- **WHEN** a line `not json` is written between two valid requests
- **THEN** the server answers the first request, an error with code `-32700`, and the second request

### Requirement: Streamable HTTP transport
`server.serve_http(app, path)` SHALL attach the server to a `use web` application at one endpoint: `POST` of a JSON-RPC message SHALL answer with `application/json` (a notification gets `202` with no body); a `GET` SHALL answer `405` unless server-initiated messages are used, which this version does not do; `DELETE` SHALL end a session. Sessions SHALL be identified by the `Mcp-Session-Id` header assigned at `initialize`, a request carrying an unknown session id SHALL get `404`, and the `MCP-Protocol-Version` header SHALL be checked when present. The server SHALL validate the `Origin` header against an allowed list (default: only same-host origins) and answer `403` otherwise, and when `bearer_token` is set SHALL answer `401` with `WWW-Authenticate: Bearer` to a missing or wrong token (compared in constant time). `serve_http` SHALL return an error. The web server SHALL have an address to listen on (`host`, empty meaning every network interface, as before), and `serve_http` SHALL move an application whose `host` is empty to `127.0.0.1`, so the endpoint is reachable from this machine only by default. For any other address (including `0.0.0.0`) `serve_http` SHALL return an error of kind `InvalidInput`, and attach nothing, unless `bearer_token` was set before the call; `127.0.0.0/8` addresses and `localhost` are loopback addresses.

#### Scenario: Session
- **WHEN** a client posts `initialize` without a session and then `tools/list` with the returned `Mcp-Session-Id`
- **THEN** the second call succeeds, and a third call with another id gets `404`

#### Scenario: Bad origin
- **WHEN** a browser page on another origin posts to the endpoint
- **THEN** the server answers `403` and runs nothing

#### Scenario: Token
- **WHEN** `bearer_token` is `secret` and a request carries no `Authorization` header
- **THEN** the answer is `401`

#### Scenario: Loopback by default
- **WHEN** `serve_http` is called on a web application whose `host` is empty
- **THEN** `host` becomes `127.0.0.1`, and a connection to the machine's network address on that port is refused

#### Scenario: Another address needs a token
- **WHEN** `serve_http` is called with `host` set to `0.0.0.0` or to a network address and `bearer_token` is empty
- **THEN** it returns an `InvalidInput` error that names `bearer_token`, and with `bearer_token` set it succeeds

### Requirement: HTTP client
`mcp.connect(url)` (with an optional bearer token) SHALL perform the handshake against a Streamable HTTP server and return an `mcp.Client` with `tools()`, `call(name, args)`, `resources()`, `read(uri)`, `prompts()`, `prompt(name, args)` and `close()`. It SHALL send the session and protocol version headers, accept both `application/json` and `text/event-stream` replies to a `POST` (collecting events until the response for the request arrives), and map JSON-RPC errors to errors of the standard type. `client.ai_tools()` SHALL return the remote tools as `ai.Tool` values (with the remote schema as given, arguments passed through as JSON) that `client.run` can use alongside local ones.

#### Scenario: Remote tool in an agent
- **WHEN** a program connects to a Zila Lang MCP server over HTTP, adds `ai_tools()` to an agent run and the fake model calls one of them
- **THEN** the remote handler runs and its text goes back to the model

#### Scenario: Event-stream reply
- **WHEN** a server answers a `POST` with `text/event-stream` containing a log notification and then the response
- **THEN** the client returns the response and ignores the notification

### Requirement: Limits and safety
A server SHALL limit message size (default 4 MiB, an error answer) and the number of requests being handled at the same time (`max_concurrent`, default 32, error `-32000` beyond it). A time limit for a tool call is not part of this version. Tool output SHALL be returned as given; the library SHALL NOT add or execute anything on its own, and SHALL NOT expose a tool, resource or prompt the program did not register.

#### Scenario: Oversized message
- **WHEN** a client posts a message larger than the limit
- **THEN** the server answers an error without running anything
