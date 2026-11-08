## Why

Text is "bytes (UTF-8)" but the language hides that in the wrong places. `s[i]` returns a byte, so `"ação"[2]` yields half of a character; `s.slice(a, b)` happily cuts a character in two and produces text that is not valid UTF-8; `reverse` reverses bytes and corrupts every accented word; `pad_left` counts bytes, so a Portuguese column does not line up; and there is no way to walk the characters of a text or turn a code point into text except the one-off `chr(n)`. A Brazilian language that cannot handle `ação` correctly is broken at its core. The fix is to make the model explicit: `text` is immutable UTF-8, `len` counts bytes, `char` is a Unicode code point, and the only ways to get at the inside of a text are ones that cannot break a character.

## What Changes

- **BREAKING** `s[i]` on a `text` is removed. The compiler rejects it with a message that points to `s.bytes()`, `s.slice(a, b)` and `for c in s`.
- **BREAKING** `slice(a, b)` takes byte positions and aborts with a message naming the position when `a` or `b` is out of range, when `a > b`, or when either is not on a character boundary. The result is always valid UTF-8.
- **New type `char`**: a Unicode scalar value (32 bits, `0..=0x10FFFF` without the surrogates). A char literal such as `'ç'` has type `char`. It compares with `==`, `<`, etc. and is not implicitly an `int`.
- `for c in s` iterates the characters of `s` (`c: char`), decoding UTF-8. Bytes that are not valid UTF-8 (possible only in text read from outside) decode as U+FFFD, one per bad byte, so the loop never aborts.
- New built-in methods: `s.len` (documented as the byte count, O(1)), `s.bytes() -> []byte` (a copy), `s.byte(i) -> byte` (O(1), aborts out of range; the primitive parsers need), `s.char_count() -> int` (O(n)).
- New conversions: `char(n)` (aborts when `n` is not a scalar value), `c.code -> int`, `c.to_text() -> text`, `c.len_utf8() -> int`, `c.is_ascii() -> bool`, and `text.from_bytes(b: []byte) -> text or error` (validates UTF-8). The one-off built-in `chr(n)` is removed in favor of `char(n).to_text()`.
- **`std/text.br`, `std/format.br`, `std/float.br`, `std/net.br`, `std/ui.br`, `std/bufio.br`, `std/tls.br`, `std/sys.br`, `os/kernel.br`, `examples/` and the tests** are rewritten to stop indexing text. `reverse` now reverses characters; `pad_left`/`pad_right` count characters and take a one-character `fill`; new `text.chars(s) -> []char` and `text.from_chars(cs) -> text`.
- Docs and README state the model once, plainly (bytes for `len`, characters for `for`, `slice` by bytes).

## Capabilities

### New Capabilities
- `text-and-char`: the `char` type, what `len`, `slice`, `bytes`, `byte`, `char_count` and `for c in s` mean for `text`, and the removal of text indexing.

### Modified Capabilities
- `text-library`: `reverse` and padding work on characters, `chars` and `from_chars` are added, and the library no longer relies on text indexing.

## Impact

- **Compiler** (`src/codegen_x64.rs`): `Ty` gets a `Char` variant; the `ExprKind::Char` case (around line 2710) returns it; `ExprKind::Index` on `Ty::Text` (around line 2941, `rt_text_byte`) becomes a compile error; `for_loop` (line 2475, which today only accepts ranges, maps and lists) gets a text branch; method dispatch near the `slice` case (line 4429) gets `bytes`, `byte`, `char_count`; the `chr` builtin (line 4289) is removed; `rt_text_slice` (line 6584) gains boundary checks. `src/lexer.rs` `char_lit` already reads a whole code point and needs no change. `src/lsp.rs`, `src/docs.rs` and `src/fmt.rs` only if they name `chr` or text indexing.
- **Standard library and OS**: the files listed above (about 70 places index a text or call `slice`).
- **Tests**: new `tests/text_char.rs`, plus the existing test programs that contain `s[i]` on a text (`tests/stdlib.rs`, `format.rs`, `buffered_io.rs`, ...), which are found by the new compile error and rewritten.
- **Docs**: `docs/syntax.html`, `docs/stdlib.html`, `docs/examples.html`, `README.md`.
- **Assumptions** (no clarification needed):
  - `s.byte(i)` is kept as a public method even though `s[i]` goes: it is O(1) and does not copy, which `bytes()` cannot offer, and it cannot break a character because it yields a number, not text. It may be dropped later if `bytes()` becomes a view.
  - No normalization, grapheme or case-mapping support: a character is a code point. `upper`/`lower` stay ASCII-only.
  - FFI is untouched: `c.char` stays the 8-bit C type; `char` is the Zila Lang type.
- **Depends on** `remove-panic-add-abort` (abort semantics), `add-direct-pe-output` and `remove-interpreter` (this change does not port anything to the interpreter; if it still exists when this lands, its text support is deleted rather than updated).
