## Purpose

Resolves host names without blocking the other tasks: `net.lookup`, and `connect` and `send_to` by name.

## Requirements

### Requirement: Looking up a host
`net.lookup(host)` SHALL return the first IPv4 address of `host` as text (`a.b.c.d`), or an error whose message is `cannot resolve '<host>'` when the name has no IPv4 address or cannot be resolved. A numeric IPv4 address SHALL be returned as it is, without asking the resolver. The call SHALL be awaitable and, while the resolver works, the task that asked SHALL be parked and every other task (timers, I/O, other lookups) SHALL keep running.

#### Scenario: A name
- **WHEN** a program awaits `net.lookup("localhost")`
- **THEN** it gets an IPv4 loopback address (`127.0.0.1`)

#### Scenario: A numeric address
- **WHEN** a program awaits `net.lookup("10.1.2.3")`
- **THEN** it gets `10.1.2.3` and the resolver is not used

#### Scenario: A name that cannot be resolved
- **WHEN** a program awaits `net.lookup` of a malformed name such as `"a b c"`
- **THEN** it gets an error `cannot resolve 'a b c'`

### Requirement: Other tasks keep running during a lookup
While a task waits for a lookup, other tasks SHALL run: a ticker task that sleeps in a loop SHALL keep ticking, and several lookups SHALL be able to be in flight at once.

#### Scenario: Concurrent lookups and a ticker
- **WHEN** one task awaits five lookups of `localhost` one after another while another task ticks every few milliseconds, and two more tasks each await a lookup at the same time
- **THEN** all lookups succeed with the right address, the ticker has run, and the program finishes

### Requirement: connect and send_to do not block on names
`net.connect(host, port)` and `UdpSocket.send_to(host, port, data)` SHALL resolve a host name with the same asynchronous lookup, so that waiting for DNS does not stop other tasks. Their results and errors SHALL otherwise be the same as before.

#### Scenario: Connecting by name
- **WHEN** a client connects to `"localhost"` and a listener on a free port accepts it, and they exchange a message
- **THEN** the exchange works as with `127.0.0.1`

#### Scenario: UDP by name
- **WHEN** a UDP socket sends to `"localhost"` and a port, and another socket bound there receives
- **THEN** the datagram arrives

#### Scenario: An unresolvable name
- **WHEN** `net.connect` or `send_to` is given a malformed name
- **THEN** it returns the error `cannot resolve '<host>'`

### Requirement: Deadlines
`timeout(ms, net.lookup(host))` SHALL work like a timeout of any other future.

#### Scenario: A lookup that finishes in time
- **WHEN** a program awaits `timeout(2000, net.lookup("localhost"))`
- **THEN** it gets the address

### Requirement: The scheduler is unchanged for everything else
Timers, files, TCP and UDP SHALL behave as before: every existing asynchronous test SHALL still pass, and a program that waits only on timers SHALL still sleep without busy-waiting.

#### Scenario: Regression
- **WHEN** the test suites for async, files, TCP, UDP and buffered I/O run
- **THEN** all pass

### Requirement: No leaks
Lookups SHALL free their memory (including the resolver's result list) and SHALL NOT leave operations pending.

#### Scenario: Repeated lookups
- **WHEN** a program looks up `localhost` thousands of times
- **THEN** `live_objects()` does not grow
