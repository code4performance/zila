## ADDED Requirements


### Requirement: Opening a TLS connection
`tls.connect(host, port)` SHALL return a future of `tls.Conn or error`. It SHALL connect with `net.connect`, run the TLS handshake with the host name as the expected server name, and fail with an error that names the host when the connection or the handshake fails. It SHALL NOT block the other tasks while it waits for the network.

#### Scenario: A public HTTPS server
- **WHEN** a program connects to `example.com` on port 443, writes an HTTP request and reads until the server closes
- **THEN** it receives a response that starts with `HTTP/1.1`

### Requirement: Certificate validation
The handshake SHALL validate the server certificate chain against the Windows trust store and check that it matches the host name, with no way to turn validation off. An expired, self-signed, untrusted or wrongly named certificate SHALL make `connect` return an error.

#### Scenario: Expired certificate
- **WHEN** a program connects to a server whose certificate has expired
- **THEN** `connect` returns an error and no data is exchanged

### Requirement: Reading and writing
`conn.write(data)` SHALL encrypt and send all of `data`, splitting it into records no larger than the negotiated maximum. `conn.read(max)` SHALL return up to `max` decrypted bytes, waiting for more records when needed, and SHALL return empty text when the server closed the connection. Bytes that arrive together with the end of the handshake SHALL NOT be lost. A connection cut in the middle of a record SHALL be reported as an error.

#### Scenario: Large and small reads
- **WHEN** a response larger than `max` is read with repeated `read(max)` calls
- **THEN** the pieces concatenate to the whole response in order

### Requirement: Closing
`conn.close()` SHALL release the security context and credentials and close the socket. A connection that is no longer referenced SHALL release them too. Reading after the connection is closed SHALL return empty text and writing SHALL return an error.

#### Scenario: Write after close
- **WHEN** a program writes to a connection after `close()`
- **THEN** `write` returns the error `write: the connection is closed`

### Requirement: Same behavior on Windows and Linux
`use tls` SHALL offer the same functions, results and error messages on Windows (SChannel) and on Linux (OpenSSL 3, `libssl.so.3`, through memory buffers so that waiting never blocks other tasks). Both SHALL validate the certificate chain and the host name. A program that uses `tls` on Linux SHALL be dynamically linked. In a bare-metal kernel `use tls` SHALL be rejected at compile time.

#### Scenario: Same program, same output
- **WHEN** a program that makes an HTTPS request and connects to a server with an expired certificate is built for Windows and for Linux and run
- **THEN** both print the same text: the start of the response, and that the bad certificate was refused

#### Scenario: Kernel
- **WHEN** a program that uses `tls` is built as a bare-metal kernel
- **THEN** compilation fails with a message that says `tls` needs Windows
