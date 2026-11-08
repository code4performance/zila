## ADDED Requirements

### Requirement: Upgrading an open connection
`tls.upgrade(conn, host)` SHALL run the TLS handshake over an existing `net.Conn` that the caller has already used for a plaintext exchange (the PostgreSQL `SSLRequest`), with `host` as the expected server name and the same certificate and name validation as `tls.connect`, and return `tls.Conn or error`. Bytes the peer sent after the plaintext exchange and before the handshake began SHALL NOT be lost or misread, and on failure the underlying connection SHALL be closed. It SHALL behave the same on Windows and Linux.

#### Scenario: Start TLS after a plaintext greeting
- **WHEN** a program writes one plaintext byte to a server that answers `S` and then upgrades the connection
- **THEN** the handshake succeeds and the program can read and write encrypted data

#### Scenario: Untrusted certificate
- **WHEN** the server presents a self-signed certificate
- **THEN** `upgrade` returns an error and the connection is closed

### Requirement: ALPN
`tls.connect_with(host, port, opts)` and `tls.upgrade_with(conn, host, opts)` SHALL accept `opts.alpn`, a list of protocol names to offer, and the resulting connection SHALL report the protocol the server selected through `conn.protocol()` (empty text when none). If the server selects none of the offered protocols the connection SHALL still be returned unless `opts.require_alpn` is true, in which case the connection SHALL fail with an error that names the offered protocols. `tls.connect(host, port)` SHALL behave as before.

#### Scenario: Server selects a protocol
- **WHEN** a program offers `tds/8.0` and `http/1.1` to a server that supports `http/1.1`
- **THEN** `conn.protocol()` is `http/1.1`

#### Scenario: Required and absent
- **WHEN** `require_alpn` is true and the server selects no protocol
- **THEN** `connect_with` returns an error naming the offered protocols

### Requirement: Private certificate authorities
`tls.connect_with` and `tls.upgrade_with` (`tls.upgrade` with the same options as `tls.connect_with`; `tls.upgrade(conn, host)` uses none) SHALL accept `ca_file`, the path of a PEM file whose certificates are trusted as additional roots for that one connection, on top of the system trust store. The chain and the host name SHALL still be validated; a certificate not signed by the system roots or by `ca_file`, an expired one, or one for another name SHALL fail as before. An unreadable or invalid `ca_file` SHALL be an error naming the path. Nothing SHALL be installed in or changed in the system trust store, and no option SHALL turn validation off.

#### Scenario: Private CA
- **WHEN** a server presents a certificate signed by a private CA and the program passes that CA's PEM as `ca_file`
- **THEN** the connection succeeds, and without `ca_file` it fails

#### Scenario: Wrong name still fails
- **WHEN** the same server is reached under a host name the certificate does not list, with `ca_file` set
- **THEN** the connection fails

#### Scenario: Bad file
- **WHEN** `ca_file` names a file that does not exist
- **THEN** the call returns an error that names the path
