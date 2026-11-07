# error-kinds Specification

## Purpose
TBD - created by archiving change add-error-kind. Update Purpose after archive.

## Requirements

### Requirement: Structure of an error
An `error` value SHALL carry a `kind` (an `ErrorKind`), a `message` (text) and a `cause` (an `error?`, nil when the error has no cause). `T or error`, `-> error`, `try`, `match` with `ok(v)` / `err(e)` and `??` SHALL keep their present meaning. `e.message` SHALL be the text given when the error was created, without the cause.

#### Scenario: Reading the parts
- **WHEN** a program creates `error(errors.NotFound, "no such user")` and reads `e.kind`, `e.message` and `e.cause`
- **THEN** the kind equals `errors.NotFound`, the message is `no such user` and the cause is nil

#### Scenario: Existing code keeps working
- **WHEN** a function returns `error("cannot parse 'x'")` and the caller matches `err(e)` and prints `e.message`
- **THEN** it prints `cannot parse 'x'` and `e.kind` equals `errors.Other`

### Requirement: Open error kinds
`ErrorKind` SHALL be a built-in type whose values are namespaced identifiers of the form `package.Name` (letters, digits and `_`, one dot). Two kinds SHALL be equal exactly when their names are equal, and `kind.name` SHALL give the name. `errors.kind(name)` SHALL create a kind from a name and SHALL be an error of kind `errors.InvalidInput` when the name is not of that form. A plain `text` SHALL NOT be accepted where an `ErrorKind` is expected, and an `ErrorKind` SHALL NOT be accepted where `text` is expected without `.name`. Libraries SHALL be able to define kinds without changing the standard library, and a kind that a program has never seen SHALL still compare, print and propagate correctly.

#### Scenario: A library defines its own kind
- **WHEN** a package declares `let Corrupt = errors.kind("mylib.Corrupt") ?? errors.Other` and returns `error(Corrupt, "bad header")`
- **THEN** a caller can test `e.kind == mylib.Corrupt` and `e.kind.name` is `mylib.Corrupt`

#### Scenario: Different namespaces never collide
- **WHEN** two packages each define a kind named `NotFound` as `a.NotFound` and `b.NotFound`
- **THEN** the two kinds are not equal, and neither equals `errors.NotFound` (whose name is `std.NotFound`)

#### Scenario: Bad name
- **WHEN** a program calls `errors.kind("no namespace")`
- **THEN** the result is an error of kind `errors.InvalidInput` whose message names the text

#### Scenario: Text is not a kind
- **WHEN** a program passes the text `"NotFound"` to `error(kind, "msg")`
- **THEN** compilation fails with a message saying an `ErrorKind` is expected and that `errors.kind(...)` makes one

### Requirement: Standard kinds
The package `errors` SHALL provide these kinds, named `std.<Name>`: `NotFound`, `PermissionDenied`, `AlreadyExists`, `Unauthenticated`, `Unavailable`, `Timeout`, `ConnectionRefused`, `ConnectionReset`, `AddrInUse`, `InvalidInput`, `Overflow`, `UnexpectedEof`, `Closed`, `Cancelled`, `Unsupported`, `Io` (an operating-system failure with no more specific kind) and `Other` (anything else, and the kind of `error("msg")`). Programs SHALL be able to add kinds but SHALL NOT be able to create kinds in the `std` namespace: `errors.kind("std.X")` called from outside the standard library SHALL be an error of kind `errors.InvalidInput`.

#### Scenario: Printing a kind
- **WHEN** a program prints `errors.Timeout.name`
- **THEN** it prints `std.Timeout`

#### Scenario: The std namespace is reserved
- **WHEN** a program calls `errors.kind("std.Mine")`
- **THEN** the result is an error of kind `errors.InvalidInput`

### Requirement: Constructors and wrapping
`error(msg)` SHALL create an error of kind `errors.Other` with no cause. `error(kind, msg)` SHALL create an error with that kind and no cause. `error(kind, msg, cause)` SHALL create an error with that kind and the given `error` as its cause. `e.context(msg)` SHALL create an error with the kind of `e`, the message `msg` and `e` as its cause; `e` itself SHALL NOT change. `e.is(kind)` SHALL be true when `e.kind` or the kind of any error in its cause chain equals `kind`. `e.full_message` SHALL be the messages of the error and its causes, outermost first, joined with `: `.

#### Scenario: Context keeps the kind
- **WHEN** `load` fails with `error(errors.NotFound, "a.json: not found")` and its caller returns `e.context("loading the config")`
- **THEN** the new error has kind `errors.NotFound`, message `loading the config` and a cause with message `a.json: not found`, and `full_message` is `loading the config: a.json: not found`

#### Scenario: Explicit cause with a different kind
- **WHEN** a library returns `error(Corrupt, "bad header", e)` for an `e` of kind `errors.UnexpectedEof`
- **THEN** `err.kind == Corrupt`, `err.cause` is `e`, `err.is(errors.UnexpectedEof)` is true and `err.is(errors.NotFound)` is false

#### Scenario: Chain of three
- **WHEN** an error is wrapped twice with `context`
- **THEN** `full_message` lists three messages and following `cause` twice reaches the root error, whose `cause` is nil

### Requirement: try carries errors unchanged
`try` SHALL propagate the failure value as it is: the same kind, message and cause. It SHALL NOT convert, wrap or add context. `call() ?? x` SHALL discard the error, as before. Adding context SHALL be explicit, through `context` or the three-argument `error`.

#### Scenario: Propagation through two functions
- **WHEN** `a` calls `try b()`, `b` calls `try await fs.read("missing.txt")`, and `main` matches `err(e)` on `a()`
- **THEN** `e.kind == errors.NotFound` and `e.message` is the message `fs.read` produced

### Requirement: Errors are reference counted with their causes
An error SHALL be released when its last reference goes away, together with its cause chain, with no leak and no double release, including when it is returned through several `try` levels, stored in a record or a list, or wrapped. A value read from `e.cause` SHALL stay valid after `e` itself goes away.

#### Scenario: Loop without leaks
- **WHEN** a program creates, wraps twice and discards a million errors in a loop
- **THEN** heap use at the end equals heap use at the start

### Requirement: Standard library failures have kinds
The standard library SHALL give these kinds, on every supported operating system, to the failures below. Messages SHALL NOT change. Failures not listed SHALL have kind `errors.Io` when they come from an operating-system call and `errors.Other` otherwise.

| Failure | Kind |
|---|---|
| `fs` open, read or write of a path that does not exist (including a missing directory) | `NotFound` |
| `fs` access denied | `PermissionDenied` |
| creating something that already exists where it must not | `AlreadyExists` |
| `Reader.read_line` at the end of input | `UnexpectedEof` |
| use of a closed file, connection or socket | `Closed` |
| `net.connect` / `tls.connect` to a port nobody listens on | `ConnectionRefused` |
| a connection reset or aborted by the peer | `ConnectionReset` |
| `net.listen` / `net.udp` on a port already in use | `AddrInUse` |
| a host name that does not resolve | `NotFound` |
| `sorted` lookups with no result (`min`/`max` of an empty collection, no key at or below/above) | `NotFound` |
| an operating-system or socket timeout, and the failure of `timeout(ms, f)` when the time passes first | `Timeout` |
| a bad argument (`text.parse_int` of non-digits, a malformed address, a bad layout in `fmt`, a zero divisor in `math.checked_div`) | `InvalidInput` |
| an integer result that does not fit (`math.checked_add`, `checked_sub`, `checked_mul`, `checked_div`) | `Overflow` |
| TLS handshake, certificate or record failure | `Io` |
| an operation canceled by the system | `Cancelled` |
| a feature the platform lacks (for example TLS renegotiation) | `Unsupported` |

#### Scenario: Missing file
- **WHEN** a program reads a file that does not exist with `try await fs.read(path)` and inspects the error
- **THEN** `e.kind == errors.NotFound` on Windows and on Linux

#### Scenario: Refused connection
- **WHEN** a program connects to `127.0.0.1` on a port with no listener
- **THEN** the error has kind `errors.ConnectionRefused`

#### Scenario: Port in use
- **WHEN** a program binds a port already held by another listener
- **THEN** the error has kind `errors.AddrInUse`

#### Scenario: End of input
- **WHEN** `read_line` is called after the last line
- **THEN** the error has kind `errors.UnexpectedEof` and message `end of file`

#### Scenario: Parser input
- **WHEN** `text.parse_int("4x2")` fails
- **THEN** the error has kind `errors.InvalidInput` and the same message as before

#### Scenario: Unmapped system error
- **WHEN** an operating-system call fails with a code that has no mapping
- **THEN** the error has kind `errors.Io` and the message `sys.describe` produced
