## 1. The error object and ErrorKind (compiler)

- [x] 1.1 Grow the error object to `[message][kind][cause]` in `src/codegen_x64.rs`: the `error(...)` call case (`name == "error"`), `rt_drop_error` (release message, kind and non-zero cause). Verify: `cargo test --test codegen` still passes and a new loop test in `tests/error_kinds.rs` (create, wrap twice, discard, compare `heap_bytes`-style leak counters used by `tests/udp.rs`) shows no leak (done: object is [message][kind][cause] (24 bytes); `rt_drop_error` releases all three; leak loop in `tests/error_kinds.rs::creating_wrapping_and_discarding_errors_leaks_nothing` uses `live_objects()` (`heap_bytes` is bare-metal only))
- [x] 1.2 Add the `ErrorKind` type (a distinct built-in type over an interned text), its `==`/`!=`, `.name`, and the `rt_kind_intern` runtime helper. Verify: tests for equality of equal names, inequality across namespaces, and that a text passed where a kind is expected is a compile error naming `errors.kind(...)` (done: new `Ty::Kind`; interning in `rt_kind_intern`; `==` is pointer comparison)
- [x] 1.3 Constructors: `error(msg)` stores static `std.Other`; add `error(kind, msg)` and `error(kind, msg, cause)` with `self.check` on every argument. Verify: spec scenarios "Reading the parts" and "Existing code keeps working"
- [x] 1.4 Field access and methods on `Ty::Err` (next to the `message` arm): `kind`, `cause` (type `error?`, nil when empty, flow typing as for other optionals), `context(msg)`, `is(kind)` (`rt_error_is`), `full_message` (`rt_error_full`). Verify: scenarios "Context keeps the kind", "Explicit cause with a different kind", "Chain of three"
- [x] 1.5 Confirm `try` and `??` need no change (the failure object is passed on as is), and that `match` `err(e)` binds the same object. Verify: scenario "Propagation through two functions" and a test that the object returned through three `try` levels is pointer-equal (same kind, message, cause)
- [x] 1.6 `src/lsp.rs`: add `kind`, `cause`, `message`, `context`, `is`, `full_message` to completion/hover for an `error` if the server lists members. Verify: `cargo test --test lsp` with a case for `e.` completion (done: the server lists no members (diagnostics and formatting only): nothing to add; `cargo test --test lsp` passes)
- [x] 1.7 Check `src/fmt.rs` and `src/docs.rs` need no change for `ErrorKind` (no new syntax). Verify: `cargo test --test fmt --test docs`

## 2. The errors package

- [x] 2.1 Create `std/errors.br` (`package errors`): the fourteen standard kinds as package-level `let` values, internal `__std(name)`, and `kind(name) -> ErrorKind or error` with the name-format check and the reserved `std` namespace check. Register the file wherever `std/*.br` packages are listed for `use` (find with `grep -rn "fs.linux.br\|include_str" src/`). Verify: scenarios "A library defines its own kind", "Bad name", "The std namespace is reserved", "Printing a kind"
- [x] 2.2 Check `use errors` works with no other `use`, and that `error("msg")` works in a program that does not `use errors`. Verify: two small programs in `tests/error_kinds.rs`

## 3. Existing code and tests

- [x] 3.1 Run every test (async skipped, per project practice) before touching the library and fix anything that depended on the 8-byte layout (`grep -rn "rt_drop_error\|Ty::Err" src/`). Verify: `cargo test` green except `tests/async.rs`
- [x] 3.2 Update `examples/errors.br` to show a kind check, `context` and a custom kind. Verify: `tests/docs.rs` (examples are compiled) and the example runs
- [x] 3.3 `src/interp.rs` is not extended. If `remove-interpreter` has not landed, make the interpreter reject `.kind`, `.cause`, `.context`, `.is`, `.full_message` with a clear "not supported by the interpreter" message and keep its tests passing. Verify: `cargo test --test parse --test testrun` (done: not applicable: `src/interp.rs` was removed by `remove-interpreter`)

## 4. Standard library kinds

- [x] 4.1 `std/sys.br` and `std/sys.linux.br`: add `kind_of(code)` and `error_for(code, what)` using the tables in the design (Windows codes, Linux errno), keeping the `describe` text. Verify: unit-style Zila Lang test calling `sys.error_for` with 2 (Windows) / 2 (Linux), 5/13, 10061/111 and an unmapped code, checking `kind` and that `message` equals `sys.describe` of the same input
- [x] 4.2 `std/fs.br` and `std/fs.linux.br`: replace `error(sys.describe(...))` with `sys.error_for(...)`; `end of file` -> `UnexpectedEof`; closed handle -> `Closed`. Verify: scenarios "Missing file" and "End of input" on Windows and Linux (`tests/buffered_io.rs`, `tests/linux.rs`), existing messages unchanged
- [x] 4.3 `std/net.br`, `std/net.linux.br`: connect/listen/udp/accept/read/write through `error_for`; unresolved names `NotFound`; closed socket and connection `Closed`; oversized datagram `InvalidInput`; `end of file` `UnexpectedEof`; the Linux DNS client's `no answer from` -> `Timeout`. Verify: scenarios "Refused connection" and "Port in use" in `tests/async.rs`-style loopback tests (run the new ones alone, `cargo test --test error_kinds`; async.rs is skipped in routine runs)
- [x] 4.4 `std/task.br`, `std/task.linux.br`: confirm the codes that reach `error_for` (cancelled 995/125, timeouts) and map any task-level failure; if no `error(...)` is produced there, record that in the commit message and change nothing. Verify: `grep -n "error(" std/task*.br` result is recorded and the timeout/cancel cases are covered by a test through `net` (done: `grep -n "error(" std/task*.br` finds nothing; `timeout(ms, f)` now fails with `Timeout` (`future.timed_out()`), tested through `net.udp` recv; 995/125 `Cancelled` and 10060/110 `Timeout` are checked through `sys.error_for`)
- [x] 4.5 `std/tls.br`, `std/tls.linux.br`: handshake, encrypt, decrypt and stream-size failures `Io` via `error_for`; renegotiation `Unsupported`; cut-off connection `ConnectionReset`; closed `Closed`. Verify: `tests/tls.rs` (messages unchanged) plus a kind assertion for a refused TLS connect
- [x] 4.6 `std/text.br` (`parse_int`, `parse_float`, `text(template, args)`), `std/format.br` and `std/float.br`: bad input -> `InvalidInput`. Verify: scenario "Parser input" and `tests/format.rs`, `tests/floats.rs` unchanged messages
- [x] 4.7 Grep for any remaining `error("` in `std/` without a kind (`grep -rn 'error("' std/`) and decide each (`Other` is allowed only for genuinely unclassified failures). Verify: the grep output is empty or each line is justified in the commit (done: no `error("` left in `std/` without a kind; `os/kernel.br` and `examples/defer.br` keep `error("...")` (kind `Other`))

## 5. Documentation

- [x] 5.1 `docs/syntax.html` (Error handling): kind, message, cause, constructors, `context`, `is`, `full_message`, the `try` rule, custom kinds, `if e.kind == ...` instead of `match`. Verify: `cargo test --test docs` (code samples are compiled)
- [x] 5.2 `docs/stdlib.html`: a new `errors` section (standard kinds, `errors.kind`) and the kind returned by each function group (fs, net, tls, text parsers); update the interpreter limits note only if `remove-interpreter` has not landed. Verify: `cargo test --test docs`
- [x] 5.3 `README.md` (the error bullets and the `error("cannot parse...")` sample if shown with kinds) and `zila.ebnf` semantic notes: replace "error("message") creates an error; e.message reads it" with the three fields, the constructors and the `try` rule. Verify: `cargo test --test docs` and a read-through that README samples still compile

## 6. Tests and validation

- [x] 6.1 `tests/error_kinds.rs`: one test per scenario of `specs/error-kinds/spec.md` (native; Linux cases gated like `tests/linux.rs`). Verify: `cargo test --test error_kinds`
- [x] 6.2 Full suite with `tests/async.rs` left out, then the new and existing network tests that touch kinds run on their own. Verify: `cargo test` (async skipped) green
- [x] 6.3 Check `os/boot.br`, `os/kernel.br` and the other `examples/*.br` for `error(` uses (`grep -rn "error(" os examples`) and confirm they still compile. Verify: `cargo test --test os --test codegen`
- [x] 6.4 `openspec validate add-error-kind --strict`
