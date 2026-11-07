## Context

`std/net.br` creates overlapped Winsock sockets (`new_socket` calls `WSASocketW(AF_INET, SOCK_STREAM, IPPROTO_TCP, ..., WSA_FLAG_OVERLAPPED)` and attaches the handle to the completion port with `task.io_attach`), starts Winsock once (`net_init`), and runs each operation as: allocate a `task.Op` (`task.op_new`), start the call with `&op` as the OVERLAPPED, then `await_op(op, accepted, err, what)`, which submits the op, waits for the completion port to wake the task and returns the bytes moved or an error from the status. `Conn` and `Listener` are shared records with a `drop` that closes the socket. `make_addr(ip, port)` builds a `sockaddr_in`, `parse_ip` and `resolve` turn text into an address, and `sys.buffer`, `sys.Scratch` and `c.from_bytes` handle raw memory. `ws.br` declares the Winsock externs (not yet `WSARecvFrom` and `WSASendTo`).

## Goals / Non-Goals

**Goals**
- UDP with the same shape and guarantees as the TCP calls: awaitable, non-blocking, errors with messages, no leaks.
- Each datagram is one value, with its sender.

**Non-Goals**
- Connected UDP sockets, broadcast, multicast, IPv6, socket options, receiving into a reused buffer, cancelling a receive (a timed-out `recv` keeps waiting in the background until a datagram arrives or the socket closes, as a timed-out TCP read does).

## Decisions

1. **Types.** `#[shared] type UdpSocket = record sock: usize, open: bool, port: int end` with a `drop` that closes the socket (like `Conn`), and `type Packet = record data: text, host: text, port: int end`.
2. **Socket creation.** `new_socket` is generalized to `new_socket_of(kind, protocol)`; the TCP code keeps calling `new_socket()`, which calls it with `SOCK_STREAM` and `IPPROTO_TCP`. UDP uses `SOCK_DGRAM` (2) and `IPPROTO_UDP` (17), attaches the handle to the completion port the same way, and then disables the Windows behaviour in which an ICMP "port unreachable" makes the next `WSARecvFrom` fail with `WSAECONNRESET`: `WSAIoctl(s, SIO_UDP_CONNRESET = 0x9800000C, &false, 4, ...)`.
3. **`udp(port)`.** `async func udp(port: int) -> UdpSocket or error`: `net_init`, create the socket, `bind` to `make_addr(0, port)`, read the bound port back with `getsockname` (as `listen` does). Errors name the port (`cannot bind UDP port N: <reason>`).
4. **`send_to`.** `resolve(host)` (numeric addresses never block), build the address with `make_addr`, a `Wsabuf` over the text's bytes, and `WSASendTo(sock, &wb, 1, 0, 0, addr, 16, &op, 0)` waited with `await_op`. A datagram is sent whole or the call fails, so there is no loop. The text is held in a local for the duration of the call (`c.cstr` temporaries live until the end of the statement, as in `Conn.write`).
5. **`recv`.** Allocate `sys.buffer(max)` and a 16-byte sockaddr buffer, a `sys.Scratch` for the address length (16) and flags, call `WSARecvFrom(sock, &wb, 1, 0, &flags, from, &len, &op, 0)`, and wait with a variant of `await_op` that returns the status code on failure so that a truncated datagram becomes `datagram larger than N bytes` instead of the system text (an overlapped receive reports it as `ERROR_MORE_DATA`, 234; the immediate case as `WSAEMSGSIZE`, 10040, and both are handled). The sender is formatted from the sockaddr bytes (`a.b.c.d`) and the port is read from bytes 2 and 3. The data is `c.from_bytes(buffer, n)`; n = 0 gives empty text and a normal packet.
6. **Closing.** `close()` closes the socket and sets `open = false`; every operation first checks `open` and returns `error("the socket is closed")`. `drop` closes if still open.
7. **Registration.** Nothing new to register (the code lives in `net`, which is already part of the native build); `ws.br` gains the two declarations, whose `Wsabuf`, flags and address parameters follow the existing `WSARecv` and `WSASend` ones.
8. **Tests.** Native tests on the loopback interface for every scenario: request/reply between two sockets in two tasks, boundaries, empty and 60,000-byte datagrams, truncation then continue, timeout with a ticker task, double close errors, rebinding after close, port in use, the ICMP case, and a leak loop. All use port 0 so they cannot collide.

## Risks / Trade-offs

- A datagram that arrives after a timed-out `recv` gives that old receive its data, which nobody reads: the next `recv` sees the following datagram. Documented.
- `recv` allocates a buffer of `max` bytes per call (64 KiB by default); programs that receive small packets in a loop can pass a smaller `max`.
- Loopback delivery order is not guaranteed by UDP; the tests that check order send with small pauses or one at a time.
