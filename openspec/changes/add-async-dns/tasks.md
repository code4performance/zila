## 1. Scheduler

- [ ] 1.1 Declare `GetQueuedCompletionStatusEx` and make `poll_io` use it with an alertable wait (completions, routine runs and timeouts handled)
- [ ] 1.2 The lookup completion routine and its race with abandoned lookups
- [ ] 1.3 Re-run the async, files, TCP, UDP and buffered I/O suites against the new wait

## 2. Lookup

- [ ] 2.1 Move the UTF-16 conversion to `sys` and use it from `fs`
- [ ] 2.2 Declare `GetAddrInfoExW` and `FreeAddrInfoExW` in `std/ws.br`
- [ ] 2.3 `net.lookup(host)` (numeric short-cut, asynchronous and synchronous completion, the result list, the error message)
- [ ] 2.4 Make `resolve` asynchronous and update `connect` and `send_to`

## 3. Tests and documentation

- [ ] 3.1 `tests/dns.rs`: every scenario of the spec (localhost, numeric, malformed name, concurrent lookups with a ticker, connect and send_to by name, timeout, leaks)
- [ ] 3.2 `docs/async.html` (networking section and the limits list) and README rows
- [ ] 3.3 Full test suite, including the async target, and `openspec validate add-async-dns`
