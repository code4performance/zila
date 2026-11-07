## 1. Library

- [x] 1.1 `std/tls.br`: SSPI declarations and `#[c_layout]` records, credentials, handshake loop with leftover bytes
- [x] 1.2 `read` (decrypt loop, incomplete messages, end of stream), `write` (record splitting), `close` and `drop`
- [x] 1.3 Register `tls` in the compiler (Linux layer, `ssl`/`crypto` library names) and reject it in the kernel
- [x] 1.4 `std/tls.linux.br` on OpenSSL with the same functions and errors

## 2. Tests and documentation

- [x] 2.1 `tests/tls.rs`: HTTPS request and bad certificate refused on Windows (`tests/tls.rs`) and on Linux through WSL (`tests/linux.rs`), skipped offline
- [x] 2.2 `docs/async.html` (TLS section and limits), `docs/native.html` (Linux table row), README roadmap and library list; `tests/linux.rs` checks the table, `tests/docs.rs` the pages
- [ ] 2.3 Follow-ups: `close_notify` on close, a buffered reader, server side
