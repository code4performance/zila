# crypto-library Specification

## Purpose
Gives programs the hashes, message authentication, encodings and random bytes that protocols such as PostgreSQL's SCRAM login need, without calling out to a C library.

## Requirements

### Requirement: Hashes and HMAC
`use crypto` SHALL provide `crypto.sha256(data)` and `crypto.md5(data)` returning the digest as raw bytes in a `text` (32 and 16 bytes; a `text` can hold any bytes, so it is shown through `crypto.hex` or `crypto.base64_encode`), `crypto.hmac_sha256(key, data)` returning 32 bytes, and `crypto.pbkdf2_sha256(password, salt, iterations, length)`. Results SHALL equal the published test vectors (FIPS 180-4 for SHA-256, RFC 1321 for MD5, RFC 4231 for HMAC, RFC 7914 section 11 for PBKDF2). Inputs of any length, including empty and larger than 4 GiB of total data fed in pieces, SHALL be accepted, and a streaming form (`crypto.Sha256` with `add` and `finish`) SHALL give the same digest as the one-shot form.

#### Scenario: SHA-256 vector
- **WHEN** a program hashes `abc` and writes the digest with `crypto.hex`
- **THEN** the output is `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`

#### Scenario: Streaming equals one-shot
- **WHEN** a program feeds one million `a` characters to a `Sha256` in pieces of 7 bytes
- **THEN** the digest equals `crypto.sha256` of the same text in one call

#### Scenario: PBKDF2 vector
- **WHEN** a program derives 64 bytes from `passwd`, salt `salt` and 1 iteration
- **THEN** the hex of the result starts with `55ac046e56e3089fec1691c22544b605`

### Requirement: Encodings
`crypto.base64_encode(data)` and `crypto.base64_decode(s)` SHALL implement RFC 4648 (standard alphabet with padding), `crypto.base64url_encode` and `crypto.base64url_decode` the URL-safe alphabet without padding, and `crypto.hex` and `crypto.unhex` lower-case hexadecimal. Decoding invalid input (bad character, wrong length, bad padding, odd number of hex digits) SHALL return an error, never a partial result.

#### Scenario: Round trip with binary data
- **WHEN** a program encodes the 256 byte values 0 to 255 with base64 and decodes the result
- **THEN** it gets the same 256 bytes

#### Scenario: Invalid base64
- **WHEN** a program decodes `abc` (wrong length) and `ab!d`
- **THEN** both return an error

### Requirement: Secure random bytes
`crypto.random(n)` SHALL return `n` bytes from the operating system's cryptographically secure generator (`BCryptGenRandom` on Windows, `getrandom` on Linux), or an error if it is unavailable. It SHALL NOT fall back to a predictable generator.

#### Scenario: Different and sized
- **WHEN** a program calls `crypto.random(32)` twice
- **THEN** each result has 32 bytes and the two results differ

### Requirement: Same results on Windows and Linux
Every function of `use crypto` SHALL give the same results on both targets. `use crypto` SHALL be rejected at compile time in the bare-metal kernel (it relies on the operating-system layer for randomness and raw memory), with a message that names the library.

#### Scenario: Parity
- **WHEN** the test program for the vectors above is built for Windows and Linux and run
- **THEN** both print identical output
