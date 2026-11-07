## Why

Everything in `use net` waits without blocking the other tasks except one thing: turning a host name into an address. `net.connect("example.com", 80)` and `sock.send_to("example.com", ...)` call the blocking Windows resolver, so a slow or unreachable DNS server freezes every task (timers, other connections, the whole program) until it answers or gives up, which can take many seconds. The docs list this as a known limit. Asynchronous DNS is the last non-blocking gap of the networking library.

## What Changes

- Name lookups wait like every other operation in `net`: they use the asynchronous Windows resolver (`GetAddrInfoExW`), and the task that asked is parked until the answer arrives while the others keep running. Numeric IPv4 addresses are still parsed on the spot with no lookup.
- `net.connect` and `UdpSocket.send_to` use it, so they no longer block on a host name. A new `net.lookup(host)` gives the address of a host as text (`a.b.c.d`) for programs that want to resolve names themselves, and `timeout(ms, net.lookup(host))` gives a lookup a deadline.
- To receive the answer without threads, the scheduler's wait for I/O becomes alertable: the resolver queues its completion on the thread that asked (a completion routine, written in Brasa), which marks the operation done and wakes the task, exactly as an I/O completion does. All other behaviour of the scheduler stays the same.
- Errors keep their message: `cannot resolve '<host>'`.
- Still IPv4 only. Names are sent to the resolver as given (no internationalized-name conversion).

## Capabilities

### New Capabilities

- `async-dns`: non-blocking name lookup, `net.lookup`, and its use by `connect` and `send_to`.

### Modified Capabilities

None.

## Impact

- `std/task.br` (alertable wait in `poll_io`, an operation kind completed by a routine), `std/ws.br` (`GetAddrInfoExW`, `FreeAddrInfoExW` declarations), `std/net.br` (`lookup`, an asynchronous `resolve`, callers updated), `std/sys.br` (a UTF-16 conversion shared with `fs`).
- New `tests/dns.rs`, `docs/async.html` (the networking section and the limits list), README.
