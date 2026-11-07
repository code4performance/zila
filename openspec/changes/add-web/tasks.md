## 1. Registration

- [ ] 1.1 Register `web` in the library tables of `src/codegen_x64.rs` (portable file `std/web.br`, no Linux layer) and reject it in the bare-metal build next to `net` (the check near the `Target::Bare` library list). Verify: `tests/web.rs` kernel test fails with an error naming `web`; `cargo build` succeeds.

## 2. Server core (`std/web.br`)

- [ ] 2.1 Records and types: `Method`, `Request`, `Response`, `Handler`, `Middleware`, `Server` (limits and timeouts as fields with defaults), constructors `web.server(port: 8080)`, `web.text`, `web.json`, `web.html_page`, `web.created`, `web.not_found`, `web.redirect`. Verify: unit programs in `tests/web.rs` print the expected status, headers and body.
- [ ] 2.2 Request parsing over `net.Reader`: request line, headers (case-insensitive, lower-cased map), percent-decoding, query map, `Content-Length` and chunked bodies, limits (`400`, `413`, `414`, `431`, `501`). Verify: `tests/web.rs` feeds raw requests through a loopback connection, including the chunked example and each error status.
- [ ] 2.3 Response writing with `Content-Length` and `Connection`, keep-alive rules for 1.0/1.1, `HEAD` without body. Verify: two requests on one connection get two ordered responses; `Connection: close` closes.
- [ ] 2.4 Timeouts through `task.add_timer` with a generation counter (`read_timeout`, `idle_timeout`, `write_timeout`), `408` for a started request. Verify: a client that stalls mid-head gets `408` while a second client is served at the same time.
- [ ] 2.5 Accept loop with `spawn serve(conn, app)`, `max_connections` with `503`, `app.run()`. Verify: a test with 100 client tasks times 10 requests all correct; a test with 1030 idle connections gets `503` beyond the limit.

## 3. Router, middleware, errors

- [ ] 3.1 Router: patterns with `:name` and `*rest`, static before parameter, `404`, `405` with `Allow`, `HEAD` fallback, trailing slash. Verify: `tests/web.rs` table of (pattern set, request, expected handler/status).
- [ ] 3.2 Middleware: `app.use`, folding at `run()`, wrapping `404`/`405`, `web.logger()`. Verify: order test (`a-in b-in handler b-out a-out`), short-circuit test, logger output test.
- [ ] 3.3 Error mapping by kind from `add-error-kind`, default mapper, `app.on_error`, no leakage of 5xx messages. Verify: one test per row of the kind table and a test that the 5xx body is `internal error`.
- [ ] 3.4 JSON helpers over `use json`: `req.json(T)` with `415` check and `InvalidInput` on bad bodies, `web.json(status, value)`. Verify: round-trip and invalid-body scenarios of the spec.

## 4. Client

- [ ] 4.1 URL parsing and `web.request`/`get`/`post` over `net` and `tls`, response reading (close, `Content-Length`, chunked), `max_response`, total `timeout`. Verify: loopback test against the library's own server for each framing; `https://example.com/` test skipped when offline, as `tests/tls.rs` does.
- [ ] 4.2 Redirects (limit 5, `http`/`https` only, no downgrade). Verify: loopback tests for a chain, a loop (error after 5) and an `https` to `http` redirect (error).

## 5. HTML templates

- [ ] 5.1 `web.escape`, `web.Safe`, `raw(...)`. Verify: unit program checks all five characters and that `Safe` is not escaped twice.
- [ ] 5.2 `#[block] comptime func html(raw: text) -> text`: parse text runs, `{expr}`, `{{`/`}}`, `@for`/`@if`/`@else`/`@end`; generate the builder code; compile errors at the block position for malformed templates. Verify: escaping, loop, composition and unbalanced-block scenarios in `tests/web.rs`.

## 6. Examples, tests and documentation

- [ ] 6.1 Rewrite `examples/rest_api.br` on `use web` (in-memory `Map` instead of `db`), keep `examples/types.br` consistent with the library's `Handler`/`Middleware`, add `examples/web.br` (templates and client). Verify: `tests/codegen.rs`/`tests/stdlib.rs` style test builds and runs both examples and `brasa fmt --check` passes on them.
- [ ] 6.2 `tests/web.rs` (all of the above, network tests skipped offline) and Windows/Linux parity in `tests/linux.rs` (same program, same output through WSL; Linux table check). Verify: `cargo test --test web` and `cargo test --test linux` (leave `tests/async.rs` out of routine runs).
- [ ] 6.3 Documentation: `docs/stdlib.html` section for `web` (server, router, middleware, errors, client, templates and the escaping limits), link from `docs/async.html`, `docs/native.html` Linux table row, README library list and roadmap, `brasa.ebnf` comment about `app.use`. Verify: `cargo test --test docs` passes (pages mention `use web` and its main functions).
- [ ] 6.4 `openspec validate add-web --strict` passes; full `cargo test` (without `tests/async.rs`) is green.
