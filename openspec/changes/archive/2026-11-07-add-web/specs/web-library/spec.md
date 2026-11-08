## ADDED Requirements

### Requirement: HTTP/1.1 request parsing
`use web` SHALL parse requests from a connection: request line (method, target, version), header lines, and a body framed by `Content-Length` or `Transfer-Encoding: chunked`. Header names SHALL be case-insensitive and the query string and path SHALL be percent-decoded. A malformed request line or header, a bad percent escape, or a request with both `Content-Length` and `chunked` SHALL get `400`. An unsupported method SHALL get `501`.

#### Scenario: Chunked body
- **WHEN** a client sends a `POST` with `Transfer-Encoding: chunked` whose chunks are `5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n`
- **THEN** the handler sees `req.body` equal to `hello world`

#### Scenario: Malformed request
- **WHEN** a client sends `GET /` without an HTTP version
- **THEN** the server answers `400` and closes the connection

### Requirement: Limits and timeouts
The server SHALL enforce `max_head` (default 16 KiB; `431` when exceeded, `414` for a request line over 8 KiB), `max_body` (default 1 MiB; `413`), `read_timeout` (default 10 s for a started request; `408`), `idle_timeout` (default 5 s between requests on a kept-alive connection; closed silently) and `write_timeout` (default 10 s). Timeouts SHALL use the task scheduler and SHALL NOT block other connections. All limits SHALL be configurable on the server.

#### Scenario: Oversized body
- **WHEN** a client declares `Content-Length` larger than `max_body`
- **THEN** the server answers `413` without reading the body and closes the connection

#### Scenario: Slow client
- **WHEN** a client sends half a request head and stops
- **THEN** after `read_timeout` the server answers `408` and closes, while other clients keep being served

### Requirement: Keep-alive
HTTP/1.1 connections SHALL stay open for further requests unless either side sends `Connection: close`; HTTP/1.0 connections SHALL close unless `Connection: keep-alive` was requested. Requests on one connection SHALL be answered in order.

#### Scenario: Two requests on one connection
- **WHEN** a client sends two requests on one connection without `Connection: close`
- **THEN** it receives two responses in order on that connection

### Requirement: Request and Response records
A handler SHALL have the type `type Handler = func(Request) -> Response or error`. `Request` SHALL expose `method` (the enum `Method`: `Get`, `Post`, `Put`, `Delete`, `Patch`, `Head`, `Options`), `version`, `path` (percent-decoded), `raw_path`, `query`, `headers`, `body` and `params`, and the methods `param(name)`, `query_param(name)` and `header(name)` returning `text?`. `Response` SHALL have `status`, `headers` and `body`; the library SHALL provide constructors `web.text`, `web.json_text`, `web.html_page`, `web.not_found` and `web.redirect`, and the JSON constructors `web.json` and `web.created` of the JSON helpers requirement. The server SHALL add `Content-Length` and `Connection` headers to every response.

#### Scenario: Plain text response
- **WHEN** a handler returns `web.text(200, "hi")`
- **THEN** the client receives status `200`, `Content-Type: text/plain`, `Content-Length: 2` and body `hi`

### Requirement: Routing with path parameters
`app.get`, `post`, `put`, `delete` and `patch` SHALL register a handler for a pattern such as `/users/:id`; a `:name` segment captures one path segment (available as `req.param("name")`) and a final `*name` segment captures the rest of the path. Static segments SHALL win over parameters. A path with no route SHALL get `404`; a path with routes only for other methods SHALL get `405` with an `Allow` header. `HEAD` SHALL fall back to the `GET` route and send no body. A trailing slash SHALL NOT change matching.

#### Scenario: Path parameter
- **WHEN** a route `/users/:id` is registered and a client requests `/users/42`
- **THEN** the handler's `req.param("id")` is `42`

#### Scenario: Static beats parameter
- **WHEN** routes `/users/me` and `/users/:id` exist and a client requests `/users/me`
- **THEN** the `/users/me` handler runs

#### Scenario: Wrong method
- **WHEN** only `GET /items` is registered and a client sends `DELETE /items`
- **THEN** the server answers `405` with `Allow: GET`

### Requirement: Middleware chain
`type Middleware = func(Request, next: Handler) -> Response or error` and `app.use(m)` SHALL append a middleware. Middleware SHALL run in registration order, outermost first, around every request including those answered `404` or `405`. A middleware MAY return without calling `next`, and MAY replace the request given to `next` or the response it gets back. `web.logger()` SHALL be provided and print method, path, status and elapsed milliseconds.

#### Scenario: Order
- **WHEN** middleware `a` and then `b` are registered and each appends its name to a log before and after calling `next`
- **THEN** the log reads `a-in b-in handler b-out a-out`

#### Scenario: Short circuit
- **WHEN** a middleware returns `401` without calling `next`
- **THEN** the route handler does not run and the client receives `401`

### Requirement: JSON helpers
`web.read_json(T, req)` SHALL check that the request's `Content-Type` contains `json` (otherwise it SHALL return an error of the kind `web.UnsupportedMedia`, mapped to `415`) and SHALL decode the body into `T` using `use json`, returning an `InvalidInput` error when the body is not valid for `T`. `web.json(T, status, value)` SHALL encode `value` with `use json` and set `Content-Type: application/json`, and `web.created(T, value)` SHALL do the same with status `201`; `web.json<[]Customer>(200, rows)` is the form for a generic type. These functions are `#[generated]` per type (the type cannot be forwarded through an ordinary function), their generated code names `json`, and so a program that calls them SHALL also write `use json`. Field names and attributes follow `use json`.

#### Scenario: Round trip
- **WHEN** a client posts `{"id":1,"name":"Ana"}` to a handler that does `let c = try web.read_json(Customer, req)` and returns `web.created(Customer, c)`
- **THEN** the client receives status `201` and the same JSON

#### Scenario: Invalid body
- **WHEN** a client posts `{"id":"x"}` where `id` is an `i32`
- **THEN** the server answers `400` with a JSON body naming the error and the path `$.id`

#### Scenario: Wrong content type
- **WHEN** a client posts a body with `Content-Type: text/plain` to a handler that calls `web.read_json`
- **THEN** the server answers `415`

### Requirement: Error mapping
When a handler or middleware returns an error, the server SHALL answer with the status for the error's kind: `InvalidInput` 400, `Unauthenticated` 401, `PermissionDenied` 403, `NotFound` 404, `AlreadyExists` 409, `Unavailable` 503, `Timeout` 504, `web.UnsupportedMedia` 415, any other kind or an error without kind 500. 4xx bodies SHALL be JSON `{"error": message}`; 5xx bodies SHALL be the text `internal error` and the message SHALL be written to the log only. `app.on_error(f)` SHALL replace the mapper.

#### Scenario: Not found
- **WHEN** a handler returns an error of kind `NotFound` with message `no customer 7`
- **THEN** the client receives `404` and `{"error":"no customer 7"}`

#### Scenario: Internal errors do not leak
- **WHEN** a handler returns an error of unknown kind with message `disk /var/db failed`
- **THEN** the client receives `500` and `internal error`, and the message is logged

### Requirement: Concurrent clients
`app.run()` SHALL accept connections in a loop and serve each in its own task on the single scheduler. 100 clients connected at the same time, each making requests, SHALL all receive correct responses, and a client that is slow or stalled SHALL NOT delay the others. When more than `max_connections` (default 1024) are open, new connections SHALL get `503` and be closed.

#### Scenario: A hundred clients
- **WHEN** 100 client tasks each send 10 requests with different path parameters to one server
- **THEN** all 1000 responses are correct and match their requests

### Requirement: HTTP client
`web.get(url)`, `web.post(url, body)` and `web.request(method, url, headers, body)` SHALL return `ClientResponse or error` with `status`, `headers` and `body`. `http://` URLs SHALL use `net`, `https://` URLs SHALL use `tls` with certificate and host name validation that cannot be turned off. The client SHALL read until the connection closes, `Content-Length` bytes, or the last chunk, SHALL fail when the body exceeds `max_response` (default 16 MiB) or the total time exceeds `timeout` (default 30 s), and SHALL follow up to 5 redirects to `http` or `https` URLs (`max_redirects`; `0` returns the 3xx response as it is); a redirect from `https` to `http`, or to any other scheme, SHALL be an error (`web.redirect_allowed(from, to)` is that rule). `web.request_with(opts, ...)` SHALL take the limits in a `ClientOptions` (`max_response`, `timeout`, `max_redirects`); `timeout` covers the exchange after the connection is made, all redirects together.

#### Scenario: Local round trip
- **WHEN** a program starts a server with a route returning `ok` and calls `web.get("http://127.0.0.1:PORT/")`
- **THEN** the result has status `200` and body `ok`

#### Scenario: HTTPS
- **WHEN** a program calls `web.get("https://example.com/")`
- **THEN** the result has status `200` and a body that contains `Example Domain`

#### Scenario: Redirect policy
- **WHEN** the redirect policy is asked about `https://a/` to `http://b/`, `http://a/` to `https://b/` and `http://a/` to `ftp://b/`
- **THEN** it refuses the first and the third and allows the second

### Requirement: HTML templates with automatic escaping
`html` SHALL be a block form (a `#[block] comptime func` of the library) that turns an `html ... end` block into code that evaluates to a `web.Safe` (a record whose `markup` field holds the text). The block holds markup with `{expr}` interpolation and control lines `@for x in xs`, `@if cond`, `@else` and `@end`; `{{` and `}}` give literal braces. Every interpolated value SHALL be converted to text and escaped (`&`, `<`, `>`, `"` and `'` become entities) unless it is a `web.Safe` value (the result of a nested `html` block) or is wrapped in `raw(...)`. A malformed template SHALL be a compile error at the position of the block.

#### Scenario: Escaping
- **WHEN** a template `html <p>{name}</p> end` is evaluated with `name` equal to `<script>alert(1)</script>`
- **THEN** the result's `markup` is `<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>`

#### Scenario: Loop
- **WHEN** a template contains `@for u in users`, a line `<li>{u.name}</li>` and `@end`, with two users
- **THEN** the result contains two `<li>` items in order

#### Scenario: Composition
- **WHEN** a template interpolates the result of another `html` block
- **THEN** that inner markup is inserted unescaped and its own interpolations are escaped once

#### Scenario: Unbalanced block
- **WHEN** a template has `@for` without `@end`
- **THEN** compilation fails with an error at the block's line

### Requirement: Same behavior on Windows and Linux
`use web` SHALL behave identically on Windows and Linux, since it is written only on `net`, `tls` and `task`. In a bare-metal kernel `use web` SHALL be rejected at compile time.

#### Scenario: Same program, same output
- **WHEN** a program that serves two routes and queries them with the client is built for Windows and Linux and run
- **THEN** both print the same text

#### Scenario: Kernel
- **WHEN** a program that uses `web` is built as a bare-metal kernel
- **THEN** compilation fails with an error that names `web`
