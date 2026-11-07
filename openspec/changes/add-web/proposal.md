## Why

Brasa has TCP, TLS and tasks, but writing a web service still means parsing HTTP by hand. `examples/rest_api.br` and `examples/types.br` already show the intended shape (`http.server`, `app.get("/customers/:id", ...)`, `Handler` and `Middleware` types, `app.use(...)`), and `brasa.ebnf` reserves `app.use` for it, yet none of it exists: `use http` is not a library. A standard `use web` makes the language useful for the most common kind of program, and exercises everything the other planned changes build: JSON, error kinds, comptime reflection, tasks without coloring.

## What Changes

- A new library `use web`, written in Brasa (`std/web.br`) on top of `use net`, `use tls`, `use json`, `use text` and `use task`. No change to the compiler beyond registering the library.
- An HTTP/1.1 server: request line, headers, bodies with `Content-Length` and `chunked`, keep-alive, size limits and timeouts driven by the scheduler, one task per connection.
- `Request` and `Response` records, routing with path parameters (`/users/:id`), a middleware chain (`type Middleware = func(Request, next: Handler) -> Response or error`), `app.use(...)`.
- JSON helpers: `req.json(T)` decodes the body, `web.json(status, value)` encodes a response.
- Error mapping: an error returned by a handler becomes an HTTP status from its `add-error-kind` kind (`NotFound` becomes 404, and so on), with a replaceable mapper.
- A small HTTP client: `web.get(url)`, `web.post(url, body)`, plain `http` and `https` (through `std/tls.br`), redirects, size limit, timeout.
- HTML templating: `web.html` is a comptime function that receives the raw `html ... end` block of `add-comptime-reflection` (markup with `{expr}` interpolation and `for`/`if` blocks) and generates code that builds the page as text, escaping every interpolated value.
- `examples/rest_api.br` is rewritten to use `use web` (`http` becomes `web`; the database part is replaced by an in-memory map because `db` does not exist), and `examples/types.br` keeps its `Handler`/`Middleware` types as the library's definitions. A new `examples/web.br` shows templates and the client.
- Windows and Linux behave the same; the library is rejected in the bare-metal kernel like `net`.

**BREAKING** (pre-1.0): the name `http` in `examples/rest_api.br` (never implemented) is replaced by `web`.

## Capabilities

### New Capabilities

- `web-library`: HTTP/1.1 server, router, middleware, JSON helpers, error mapping, client and HTML templates.

### Modified Capabilities

None.

## Impact

`std/web.br` (new), `src/codegen_x64.rs` (register the library, reject it in the kernel), `examples/rest_api.br`, `examples/web.br` (new), `tests/web.rs` (new), `tests/linux.rs`, `tests/docs.rs`, `docs/stdlib.html` (new section), `docs/async.html` (link), `docs/native.html` (Linux table row), `README.md` (library list and roadmap), `brasa.ebnf` comment. Depends on `add-comptime-reflection`, `add-json`, `add-error-kind` and `remove-async-coloring`; see design.md.
