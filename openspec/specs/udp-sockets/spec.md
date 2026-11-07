## Purpose

Lets programs send and receive UDP datagrams with `net.udp`, `send_to` and `recv`, without blocking other tasks.

## Requirements

### Requirement: Binding a socket
`net.udp(port)` SHALL create a UDP socket bound to `port` on all IPv4 interfaces and return a `net.UdpSocket` (or an error that names the port and the reason, for example when the port is in use). A `port` of 0 SHALL pick a free port, which SHALL be available as the socket's `port` field. Waiting on the socket SHALL NOT block other tasks.

#### Scenario: A free port
- **WHEN** a program binds `net.udp(0)` twice
- **THEN** both succeed with different non-zero ports

#### Scenario: A port in use
- **WHEN** a program binds a port that another socket of the program already holds
- **THEN** the second `net.udp(port)` is an error naming the port

### Requirement: Sending and receiving datagrams
`sock.send_to(host, port, data)` SHALL send `data` as one datagram to the IPv4 address or host name and port, and return an error when it cannot be sent (including a host name that cannot be resolved). `sock.recv(max)` SHALL wait for the next datagram and return a `net.Packet` with `data` (the datagram's bytes), `host` (the sender's address as `a.b.c.d`) and `port` (the sender's port); `max` SHALL default to 65,535. Each `recv` SHALL return exactly one datagram, whose boundaries are preserved, and a datagram with no data SHALL be a valid packet with empty `data`.

#### Scenario: Request and reply
- **WHEN** a client socket sends `ping` to a server socket on `127.0.0.1`, the server receives it and replies to the packet's `host` and `port` with `pong`
- **THEN** the server's packet has data `ping`, host `127.0.0.1` and the client's port, and the client receives `pong`

#### Scenario: Boundaries
- **WHEN** a socket sends the datagrams `a`, `bb` and `ccc` to another
- **THEN** three `recv` calls return `a`, `bb` and `ccc` separately

#### Scenario: Empty datagram
- **WHEN** a socket sends an empty text
- **THEN** the receiver gets a packet whose `data` is empty

#### Scenario: Large datagram
- **WHEN** a socket sends a datagram of 60,000 bytes
- **THEN** the receiver gets all 60,000 bytes unchanged

### Requirement: Datagrams larger than the buffer
When a datagram is larger than `max`, `recv` SHALL return an error that says the datagram was larger than `max` bytes, and the socket SHALL remain usable for the next datagram.

#### Scenario: Too small a buffer
- **WHEN** a datagram of 100 bytes is received with `recv(10)` and a short datagram follows
- **THEN** the first `recv` is an error mentioning the 10 bytes, and the next `recv` returns the short datagram

### Requirement: Deadlines and many tasks
`timeout(ms, sock.recv())` SHALL give an error that says the operation timed out when no datagram arrives in time, and a receive in one task SHALL NOT keep other tasks from running.

#### Scenario: Timeout
- **WHEN** a program awaits `timeout(50, sock.recv())` on a socket nobody sends to
- **THEN** it gets a timeout error after about 50 milliseconds and a ticker task kept running meanwhile

### Requirement: Closing
`sock.close()` SHALL close the socket at once; a socket that is dropped SHALL be closed when its last reference goes away. Calling `send_to` or `recv` on a closed socket SHALL return an error saying the socket is closed. A closed port SHALL be bindable again. An ICMP port-unreachable reply to an earlier send SHALL NOT make a later `recv` fail.

#### Scenario: Use after close
- **WHEN** a program closes a socket and then calls `recv` and `send_to` on it
- **THEN** both return an error saying the socket is closed

#### Scenario: Rebinding
- **WHEN** a socket bound to a port is closed
- **THEN** `net.udp(port)` with the same port succeeds

#### Scenario: Sending to a closed port
- **WHEN** a socket sends to a local port nobody listens on and then receives a datagram sent by another socket
- **THEN** the receive returns that datagram (it does not fail with a connection reset)

### Requirement: No leaks
Sockets and packets SHALL free their memory, and a dropped socket SHALL release its operating system handle.

#### Scenario: Repeated use
- **WHEN** a program binds, sends, receives and closes sockets thousands of times
- **THEN** `live_objects()` does not grow
