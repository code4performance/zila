## Context

`net.Conn` offers `read` and `write` of bytes as `text`. SChannel is a message-based API (SSPI): a handshake loop of `InitializeSecurityContext` calls that exchange tokens, then `EncryptMessage` / `DecryptMessage` over TLS records.

## Decisions

1. **SChannel, not a Brasa implementation.** Reuses the platform's TLS 1.2/1.3, trust store and name check, and avoids shipping home-made cryptography. Linux has no SChannel, so `std/tls.linux.br` uses OpenSSL 3 with the same API: a memory BIO pair, `SSL_do_handshake` / `SSL_read` / `SSL_write`, and the bytes in and out moved with `net` (so epoll parking keeps other tasks running); verification with `SSL_set_verify(PEER)`, `SSL_set1_host` and the default trust paths. Cost: `libssl.so.3` is needed at run time on Linux.
2. **`#[c_layout]` records for SSPI structures.** `SecBuffer`s are flattened into a four-slot record (`Bufs`) next to `Desc`; results written by the operating system are read back from the same records (same technique as `net`).
3. **Stream framing in the library.** `Conn` keeps `pending` (received bytes not yet decrypted, including data that arrived with the handshake) and `plain` (decrypted bytes not yet returned). `SEC_E_INCOMPLETE_MESSAGE` means read more; `SEC_I_CONTEXT_EXPIRED` and a closed socket end the stream; renegotiation is an error.
4. **Handles are released by `drop`** (and by `close`), using the same `#[shared]` pattern as sockets.
5. **Flags.** `ISC_REQ_ALLOCATE_MEMORY | STREAM | CONFIDENTIALITY | SEQUENCE_DETECT | REPLAY_DETECT`, with `SCH_USE_STRONG_CRYPTO | SCH_CRED_NO_DEFAULT_CREDS`; certificate validation stays automatic.

## Risks

- Cryptography runs inline on the single task thread (short blocks, acceptable).
- `close_notify` is not sent on close, so a peer cannot tell a clean close from a cut.
- Tests that use the network are skipped when offline.
