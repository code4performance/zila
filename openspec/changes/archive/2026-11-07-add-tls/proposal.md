## Why

The roadmap's first standard-library item was TLS: without it a Zila Lang program cannot talk to HTTPS services. Writing TLS (handshake, certificate validation, cipher suites) from scratch is a security-critical, very large job; Windows already ships a vetted implementation (SChannel) that the standard library can reach through `extern`, like it does for Winsock.

## What Changes

- A new library `use tls`, written in Zila Lang on `secur32.dll` (SChannel): `tls.connect(host, port)` opens a TCP connection with `net`, runs the handshake and returns a `tls.Conn` with `read(max)`, `write(data)` and `close()`.
- The server certificate is validated against the Windows trust store and the host name; an invalid certificate makes `connect` fail with an error.
- Client side only. Windows uses SChannel (`std/tls.br`); Linux uses OpenSSL 3 through `extern` and memory buffers (`std/tls.linux.br`), with the same functions and errors. The compiler maps `#[link("ssl")]` and `#[link("crypto")]` to `libssl.so.3` and `libcrypto.so.3`. In the bare-metal kernel `use tls` is rejected at compile time.
- The async documentation gains a TLS section; the Linux compatibility table lists `tls`; the roadmap row now asks for TLS on Linux and for servers.

## Capabilities

### New Capabilities

- `tls-client`: encrypted client connections.

### Modified Capabilities

None.

## Impact

`std/tls.br` and `std/tls.linux.br` (new), `src/codegen_x64.rs` (library registration, Linux layer, shared-object names), `tests/tls.rs` (new), `tests/linux.rs`, `docs/async.html`, `docs/native.html`, `README.md`. Written after the code (retroactive record).
