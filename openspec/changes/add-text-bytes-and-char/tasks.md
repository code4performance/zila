## 1. The `char` type

- [ ] 1.1 Add `Ty::Char` (4 bytes, unsigned) in `src/codegen_x64.rs`, make `ExprKind::Char` (around line 2710) and `const_value` return it, and add it to `Ty::show`, `size`, comparisons and the `Ty` matches the compiler reports as non-exhaustive; verify `let c: char = 'ç'`, `c == 'ç'`, `c < 'z'` compile and run in a test in `tests/text_char.rs`, and that `let n: int = 'a'` fails with a message naming `char` and `int`
- [ ] 1.2 Make `print`, interpolation (`"{c}"`) and `+` with text write a char as UTF-8 (a runtime routine `rt_char_to_text`); verify `print('ç')` prints `ç` and `"{'ã'}o"` is `ão`, byte for byte, on Windows and Linux
- [ ] 1.3 Add `char(n)` (abort when `n < 0`, `n > 0x10FFFF` or a surrogate; message names the value), `c.code`, `c.to_text()`, `c.len_utf8()`, `c.is_ascii()`; verify the round trip, the surrogate abort (message and exit status) and `char(0x1F600).len_utf8() == 4`
- [ ] 1.4 Allow `char` in lists, records, parameters, return values, `match` char patterns and as a `Map` key; verify one test each, and that `[]char` frees without leaks (`live_objects()` flat over 1000 rounds)

## 2. Text indexing goes away

- [ ] 2.1 Turn the `Ty::Text` arms of `ExprKind::Index` (read near line 2941 and assignment near line 2410) into the compile error `text cannot be indexed: use s.bytes() ..., s.slice(a, b) ..., or for c in s ...`; verify with a test on the exact message and the line, that `[]T` and `Map` indexing still work, and that `src/lsp.rs` reports the same message in a diagnostics test in `tests/lsp.rs`
- [ ] 2.2 Replace `chr(n)` (builtin near line 4289) by nothing, and verify `chr(65)` now fails with `unknown function chr` plus a hint to use `char(n).to_text()`
- [ ] 2.3 Run `cargo test --workspace -- --skip async` (async tests are skipped by routine runs) and list every test program that fails to compile because of 2.1 or 2.2; verify the list is saved in the scratchpad as the work list for tasks 5 to 7 (nothing is fixed here)

## 3. Text primitives in the compiler and runtime

- [ ] 3.1 Rename `rt_text_byte` to `rt_text_byte_at`, add the out-of-range abort, and expose `s.byte(i)` returning `byte`; verify `"ação".byte(1) == 195` and that `"abc".byte(3)` aborts with the position in the message
- [ ] 3.2 Add boundary checks to `rt_text_slice` (line 6584) with the three messages from the spec (out of range, `a > b`, not on a character boundary); verify `"ação".slice(0, 3) == "aç"`, `slice(0, 2)` and `slice(1, 9)` abort with the specified messages, `slice(len, len)` is empty, and the cost is flat (slicing a 1 MB text 100000 times takes about as long as slicing a short one)
- [ ] 3.3 Add `s.bytes()` (new `[]byte` copy) and `s.char_count()` (O(n) scan, an invalid byte counts as one), next to the `slice` case near line 4429; verify the Portuguese scenario (`len` 6, `char_count` 4, `bytes().len` 6), that mutating the returned list leaves `s` unchanged, and no leaks over 1000 rounds
- [ ] 3.4 Add the `for c in s` branch to `for_loop` (line 2475) with `rt_text_next_char` and U+FFFD for invalid bytes; verify the code points of `"ação"` are `97, 231, 227, 111`, an empty text runs zero times, `break`/`continue`/`defer` inside the loop behave as in a list loop, a text built from bytes `97, 255, 98` visits `a`, U+FFFD, `b`, and reassigning the iterated variable inside the body does not break the loop
- [ ] 3.5 Add `text.from_bytes` (UTF-8 validation, error `not valid UTF-8 at byte N`) to `std/text.br` using `byte(i)`; verify valid input, an overlong encoding, a truncated sequence, a surrogate encoding and `97, 255` all behave as specified
- [ ] 3.6 Update the linker/assembly output for Linux and Windows only through the existing runtime (no new instruction forms; confirm with the assembler test from `add-direct-pe-output` if it has landed); verify the Linux suite (`tests/linux.rs`) and the bare-metal image test still pass

## 4. Standard library: text

- [ ] 4.1 Rewrite `index_from`, `starts_with`, `ends_with`, `compare`, `lines`, `is_space`, `trim_start`/`trim_end` in `std/text.br` with `s.byte(i)`; verify the existing searching, splitting, lines, trimming and compare scenarios of `tests/stdlib.rs` pass unchanged and that `split` over a 1 MB text stays linear (time test)
- [ ] 4.2 Rewrite `parse_int` and `parse_float` (the `s[0]`, `s[j]` sites) with `byte`; verify the parsing scenarios, the random-decimal comparison against Rust's parser and the print-then-parse round trip still pass
- [ ] 4.3 Replace `shift_ascii` and its `chr` use so `upper`/`lower` copy runs with `slice` and convert ASCII letters only; verify `upper("Straße 1") == "STRAßE 1"` and `lower("AÇÃO") == "aÇÃo"`
- [ ] 4.4 Rewrite `reverse` over characters, and add `chars(s)` and `from_chars(cs)`; verify `reverse("ação") == "oãça"`, `reverse(reverse(s)) == s` on a list of multibyte strings, and `from_chars(chars(s)) == s`
- [ ] 4.5 Rewrite `pad_left` and `pad_right` to count characters and abort for a `fill` that is not one character; verify `pad_right("ação", 6, ".") == "ação.."`, the number-column scenario and the abort message
- [ ] 4.6 Verify the no-leaks scenario of the text library over `split`, `join`, `replace`, `trim`, `reverse` and `pad_*` with `live_objects()`

## 5. Standard library and OS: other modules

- [ ] 5.1 Migrate `std/format.br`: replace the hand-written UTF-8 width counter at its top with `char_count()` and the layout/template parsing sites (`layout[first]`, `template[...]`, `s[i]`) with `byte(i)`; verify `tests/format.rs` and `examples/report.br` print the same table, with `Ação` aligned by characters
- [ ] 5.2 Migrate `std/float.br`, `std/bufio.br`, `std/sys.br`, `std/tls.br` and `std/tls.linux.br`, `std/ui.br`, `std/ui.linux.br`, `std/net.br`, `std/net.linux.br` (find every site with the compiler errors from task 2.3 and by `grep -n "\[[a-z_0-9 +-]*\]"` on text variables); verify `tests/floats.rs`, `tests/buffered_io.rs`, `tests/tls.rs`, `tests/ui.rs`, `tests/dns.rs`, `tests/udp.rs` pass and `tests/linux.rs` output is unchanged
- [ ] 5.3 Migrate `os/boot.br` and `os/kernel.br` (`console_write`, `read_line`, and its `chr` use); verify `tests/os.rs` and booting the image under the emulator `os/run.ps1` uses prints the prompt and echoes typed text

## 6. Examples and tests

- [ ] 6.1 Rewrite text indexing in `examples/` (`lowlevel.br`, `words.br`, `types.br`, `values.br`, `control.br`, `async.br`, `ffi.br` as the compiler reports); verify every example builds and its output matches the output before this change (compare with a saved run)
- [ ] 6.2 Add `examples/unicode.br` that prints `len`, `char_count`, characters with their code points, a safe `slice`, and `reverse` for `ação`, `coração` and `São Paulo`; verify the output file is added to the example list in `tests/codegen.rs` and passes
- [ ] 6.3 Migrate every test program in the work list from 2.3 (`tests/stdlib.rs`, `tests/format.rs`, `tests/buffered_io.rs`, `tests/generics.rs`, ...) and, where a test's job was to exercise `s[i]`, replace it with the `byte`/`for` equivalent; verify `cargo test --workspace -- --skip async` is green and no test source still contains an indexed text (`grep` over `tests/` for the compile error text returns nothing)
- [ ] 6.4 Write `tests/text_char.rs` covering every scenario of the `text-and-char` and `text-library` deltas, with Portuguese data (`ação`, `coração`, `São Paulo`, `não`), a 4-byte character (`😀`), the invalid-byte iteration and the no-leak loops; verify the file passes on Windows and, through `tests/linux.rs` helpers, on Linux

## 7. Docs

- [ ] 7.1 Update `docs/syntax.html`: a "Text and characters" section that states `len` is bytes (with `"ação".len == 6`), shows `for c in s`, `char`, `c.code`, `char(n)`, `slice` by bytes and the abort, `bytes()`, `byte(i)` and `char_count()`, and says `s[i]` does not exist and why; verify `tests/docs.rs` (navigation, links, anchors) passes
- [ ] 7.2 Update `docs/stdlib.html` (`reverse`, `pad_*`, `chars`, `from_chars`, `from_bytes`; remove "Text is bytes" paragraph and replace it with the model), `docs/examples.html` (the new example), and `README.md` (status table and a one-line note on the `s[i]` breaking change); verify `tests/docs.rs` and that no page still says `chr` or shows a text index
- [ ] 7.3 Run `brasa fmt` over the migrated `.br` files and the whole docs/test suite one final time; verify `brasa fmt --check` reports nothing and `openspec validate add-text-bytes-and-char --strict` passes
