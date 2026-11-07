## Purpose

Defines text as immutable UTF-8 and the `char` type: what `len`, `slice`, `bytes`, `byte`, `char_count` and `for c in s` mean, and why text cannot be indexed, so no operation on the safe surface can cut a character in half.

## Requirements

### Requirement: Text is immutable UTF-8
A `text` SHALL be an immutable sequence of bytes holding valid UTF-8 (a pointer and a length). `s.len` SHALL be the number of bytes, computed in constant time, and SHALL NOT be the number of characters. Literals, `slice`, `+`, interpolation and every library function SHALL produce valid UTF-8.

#### Scenario: Length counts bytes
- **WHEN** a program prints `"ação".len` and `"ação".char_count()`
- **THEN** it prints `6` and `4`

### Requirement: No text indexing
Indexing a `text` with `s[i]` SHALL be a compile-time error whose message says that text cannot be indexed and names `s.bytes()`, `s.slice(a, b)` and `for c in s` as the alternatives.

#### Scenario: Index is rejected
- **WHEN** a program contains `let b = s[0]` with `s: text`
- **THEN** compilation fails at that line with a message that starts with `text cannot be indexed` and mentions `bytes()`, `slice` and `for c in s`

### Requirement: The char type
`char` SHALL be a Unicode scalar value held in 32 bits. A char literal SHALL have type `char`. `char` values SHALL support `==`, `!=`, `<`, `<=`, `>`, `>=` by code point, SHALL NOT convert implicitly to or from `int`, and SHALL print and interpolate as the one-character text. `char(n)` SHALL convert an integer and SHALL abort when `n` is negative, above `0x10FFFF` or a surrogate (`0xD800..=0xDFFF`). `c.code` SHALL be the code point as an `int`, `c.to_text()` SHALL be the UTF-8 text of `c`, `c.len_utf8()` SHALL be 1 to 4, and `c.is_ascii()` SHALL tell whether `c.code < 128`.

#### Scenario: Round trip
- **WHEN** a program evaluates `'ç'.code`, `char(231).to_text()` and `'ç'.len_utf8()`
- **THEN** the results are `231`, `ç` and `2`

#### Scenario: Invalid code point aborts
- **WHEN** a program evaluates `char(55296)` (a surrogate)
- **THEN** it aborts with a message naming the value, and exits with the abort status

#### Scenario: No implicit int
- **WHEN** a program assigns `'a'` to an `int` variable without `c.code`
- **THEN** compilation fails with a type mismatch naming `char` and `int`

### Requirement: Iterating characters
`for c in s` over a `text` SHALL bind `c: char` to each code point in order, decoding UTF-8. Bytes that do not form valid UTF-8 SHALL decode as U+FFFD, one per offending byte, and SHALL NOT abort the loop. An empty text SHALL run the body zero times.

#### Scenario: Portuguese word
- **WHEN** a program collects `c.code` for `for c in "ação"`
- **THEN** it gets `97, 231, 227, 111`

#### Scenario: Invalid bytes
- **WHEN** a program iterates a text whose bytes are `97, 255, 98` (possible only for text read from outside the program, for example a file)
- **THEN** it visits `a`, U+FFFD and `b`

### Requirement: Slicing by bytes
`s.slice(a, b)` SHALL take byte positions, return the bytes from `a` up to but not including `b`, and SHALL abort when `a < 0`, `b > s.len`, `a > b`, or `a` or `b` is inside a multibyte character. The abort message SHALL name the offending position and say whether it is out of range or not on a character boundary. `slice(s.len, s.len)` SHALL return an empty text.

#### Scenario: Valid and invalid cut
- **WHEN** a program evaluates `"ação".slice(0, 3)` and `"ação".slice(0, 2)`
- **THEN** the first gives `aç` and the second aborts with a message saying position 2 is not on a character boundary

#### Scenario: Out of range
- **WHEN** a program evaluates `"abc".slice(1, 9)`
- **THEN** it aborts with a message saying position 9 is out of range

### Requirement: Bytes and character counts
`s.bytes()` SHALL return a new `[]byte` holding the bytes of `s`; changing the list SHALL NOT change `s`. `s.byte(i)` SHALL return the byte at position `i` in constant time and SHALL abort when `i` is out of range. `s.char_count()` SHALL return the number of code points, counting an invalid byte as one, in time proportional to `len`. `text.from_bytes(b)` SHALL return the `text` of the bytes `b` as `text or error`, with the error message `not valid UTF-8 at byte N` when they are not valid.

#### Scenario: Bytes of a word
- **WHEN** a program prints `"ação".bytes().len` and `"ação".byte(1)`
- **THEN** it prints `6` and `195`

#### Scenario: Validation
- **WHEN** a program calls `text.from_bytes` on the bytes `97, 255` and on the bytes `97, 98`
- **THEN** the first gives the error `not valid UTF-8 at byte 1` and the second gives `ab`

### Requirement: No leaks
`bytes()`, `c.to_text()`, `from_bytes` and loops over text SHALL release everything they allocate.

#### Scenario: Repeated use
- **WHEN** a program calls `bytes()`, `slice`, `char_count()` and iterates a multibyte text 1000 times
- **THEN** `live_objects()` does not grow between iterations
