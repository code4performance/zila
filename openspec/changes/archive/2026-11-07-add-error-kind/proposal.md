## Why

Today an `error` is only a message: `error("notes.txt: The system cannot find the file specified.")`. A program that wants to react to a missing file differently from a denied one has to compare text, and the text differs between Windows and Linux, between languages of the operating system and between releases of the standard library. There is also no way to say "loading the config failed *because* the file could not be read": adding context means building a new message and throwing the original away.

The shape `T or error` / `try` / `match ok(v) err(e)` is right and stays. What is missing is structure inside the error: a machine-readable kind, the message, and an optional cause.

## What Changes

- **BREAKING** `error` keeps being one type, but it now carries three things: `e.kind` (an `ErrorKind`), `e.message` (text, as today) and `e.cause` (an `error?`, nil for a root error).
- `ErrorKind` is a new built-in type: an open, namespaced identifier (`std.NotFound`, `mylib.Corrupt`), compared with `==`, with a `name` text. Libraries make their own with `errors.kind("mylib.Corrupt")`. There is no closed enum.
- A new standard package `errors` (`use errors`) holds the standard kinds as values: `NotFound`, `PermissionDenied`, `AlreadyExists`, `Unauthenticated`, `Unavailable`, `Timeout`, `ConnectionRefused`, `ConnectionReset`, `AddrInUse`, `InvalidInput`, `UnexpectedEof`, `Closed`, `Cancelled`, `Unsupported`, `Io`, `Other`.
- Constructors: `error("msg")` (kind `errors.Other`, unchanged behavior), `error(kind, "msg")`, `error(kind, "msg", cause)`; `e.context("msg")` wraps `e` in a new error that keeps its kind and has `e` as cause; `e.is(kind)` is true when the error or any cause has that kind; `e.full_message` joins the chain (`context: cause: root`).
- `try` converts nothing: kind, message and cause travel unchanged.
- The standard library returns proper kinds on the common failures: `std/fs.br`, `std/net.br`, `std/tls.br`, the parsers in `std/text.br`, `std/format.br` and `std/float.br`, `std/task.br` and the `.linux` variants, through an operating-system-code-to-kind mapping next to `sys.describe`.
- Documentation (`docs/syntax.html`, `docs/stdlib.html`, README, `brasa.ebnf` semantic notes), `examples/errors.br` and tests are updated.

## Capabilities

### New Capabilities

- `error-kinds`: the structure of `error` (kind, message, cause), the open `ErrorKind` type, the standard kinds, constructors, wrapping, `try` semantics and the kinds returned by the standard library.

### Modified Capabilities

None. No existing spec describes `error`: the specs `async-io`, `tls-client`, `udp-sockets` and `buffered-io` only say that failures are errors with messages, and those messages do not change. Only kinds are added.

## Impact

- Compiler: `src/codegen_x64.rs` (the error object layout and `rt_drop_error`, the `error(...)` call forms near the `name == "error"` case, field access in the `Ty::Err if name == "message"` arm, new methods `context`, `is`, the `ErrorKind` type), `src/lsp.rs` (completion and hover for the new fields, if it lists them), `src/parser.rs` only if `ErrorKind` needs parsing as a type name. `src/interp.rs` is not extended (see design: `remove-interpreter`).
- Standard library: new `std/errors.br`; `std/sys.br` and `std/sys.linux.br` (`error_for`), `std/fs.br`, `std/fs.linux.br`, `std/net.br`, `std/net.linux.br`, `std/tls.br`, `std/tls.linux.br`, `std/task.br`, `std/task.linux.br`, `std/text.br`, `std/format.br`, `std/float.br`.
- `examples/errors.br`, new `tests/error_kinds.rs`; existing tests that check message text keep passing.
- Docs: `docs/syntax.html` (Error handling), `docs/stdlib.html`, README, `brasa.ebnf`.
