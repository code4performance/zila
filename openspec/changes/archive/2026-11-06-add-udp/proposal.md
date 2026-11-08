## Why

`use net` speaks only TCP, so a Zila Lang program cannot do the things UDP is for: sending small independent messages without a connection (logging and metrics, game and sensor traffic, discovery on a local network, simple request/reply protocols, DNS-style lookups). UDP is the next item of the standard library roadmap, and it fits the way TCP is already done: overlapped Winsock calls waited on through the scheduler's completion port.

## What Changes

- `net.udp(port)` binds a UDP socket (0 picks a free port; the number is in `.port`) and returns a `net.UdpSocket`, as an awaitable that can fail like `net.listen`.
- `sock.send_to(host, port, data)` sends one datagram (a numeric IPv4 address or a host name, as with `net.connect`), and `sock.recv(max)` waits for the next datagram and returns a `net.Packet` with the `data` (up to `max` bytes, 65,535 by default), the sender's `host` (dotted text) and `port`. One `recv` is one datagram: boundaries are kept, an empty datagram is a valid packet with empty `data`, and a datagram larger than `max` is an error that says so (its data is dropped).
- `sock.close()` closes the socket now; sockets also close when the last reference goes away. Using a closed socket is an error. ICMP "port unreachable" replies do not break later receives.
- Like the rest of `net`: IPv4 only, Windows only, nothing blocks the other tasks (a host name still goes through the blocking resolver), and `timeout(ms, sock.recv())` gives a receive deadline.
- No connected UDP sockets, broadcast or multicast in this change.

## Capabilities

### New Capabilities

- `udp-sockets`: binding, sending and receiving datagrams, packets, errors and closing.

### Modified Capabilities

None.

## Impact

- `std/ws.br` (the `WSARecvFrom` and `WSASendTo` declarations), `std/net.br` (the socket type, `udp`, `send_to`, `recv`, `close`, a generalized socket constructor).
- New `tests/udp.rs`, `examples/udp.br`, `docs/async.html` (the networking section and the limits list), README.
