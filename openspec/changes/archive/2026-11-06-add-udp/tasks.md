## 1. Winsock and sockets

- [x] 1.1 Declare `WSARecvFrom` and `WSASendTo` in `std/ws.br`
- [x] 1.2 Generalize socket creation (`new_socket_of`), add the UDP socket type with `drop`, and disable the connection-reset behaviour of UDP sockets
- [x] 1.3 `net.udp(port)` with the bound port read back, and the port-in-use error

## 2. Sending and receiving

- [x] 2.1 `send_to(host, port, data)`
- [x] 2.2 `recv(max)` returning a `Packet` (sender host and port, empty datagrams, the too-large error and continuing afterwards)
- [x] 2.3 `close()`, the closed-socket errors and release on drop

## 3. Tests, example and documentation

- [x] 3.1 `tests/udp.rs`: every scenario of the spec on the loopback interface (native)
- [x] 3.2 `examples/udp.br`
- [x] 3.3 `docs/async.html` (networking section and the limits list) and README rows
- [x] 3.4 Full test suite (async skipped) and `openspec validate add-udp`
