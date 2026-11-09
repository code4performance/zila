## Context

`text.upper` and `lower` call `shift_ascii`, which walks bytes and shifts the ASCII letters. `for c in s` decodes a text into `char` values (code points), with bad bytes as U+FFFD, and `char(n).to_text()` encodes one back. The standard library carries no Unicode data, and the project takes no dependencies.

## Goals / Non-Goals

**Goals:** correct case for the alphabets the language's users write in; ASCII text as fast as before; no tables to load at run time; a result that can be checked against an authority.

**Non-Goals:** full Unicode (see proposal.md).

## Decisions

**Arithmetic on ranges, not a table.** Case mapping in these blocks is regular: Latin-1, Greek and Cyrillic letters are a fixed offset apart, and Latin Extended-A, Cyrillic extensions and Latin Extended Additional alternate capital and small letters (even/odd, with a few shifted runs). The mapping is two functions on integers, `upper_cp` and `lower_cp`, written as a chain of range tests. Alternative: an embedded table of pairs searched by binary search. Rejected: hundreds of lines of data to keep correct by hand, and it adds a data section to every program that uses `upper`, against the project's rule that an unused `use` costs nothing (pruning removes the functions and, with them, the code; a data table is harder to prune).

**Checked against Rust's own mapping.** The supported blocks and the exceptions are written once, in a test that maps every code point of those blocks through `upper` and `lower` in a Zila program and compares with `char::to_uppercase` and `to_lowercase` where those give a single character. That turns "I believe the ranges are right" into a test, and the test names the first code point that differs.

**Simple mapping only.** Where Unicode's full mapping gives several characters (`ß` to `SS`, `ŉ`, Greek characters with dialytika and tonos) the character is left alone, and `İ` (which lowercases to `i` plus a combining dot) lowercases to `i`. Documented in the spec and the docs, because a program that needs `SS` can special-case `ß` itself.

**Fast path for ASCII.** A first pass looks for any byte of 128 or more; without one the old byte loop runs. Otherwise the text is walked by characters and rebuilt with `concat_all` of one small text per character, like `shift_ascii` does.

**Invalid UTF-8.** `for c in s` yields U+FFFD for bad bytes, so mapping such text replaces them rather than aborting; before, `upper` aborted on them whenever it sliced next to a bad byte.

## Risks / Trade-offs

- [A range written wrong maps a letter to a different letter] → the exhaustive comparison test over the supported blocks.
- [Rebuilding by character is slower than the byte loop for long non-ASCII text] → acceptable for text-sized inputs; only non-ASCII text pays.
- [Code that depended on non-ASCII staying unchanged] → listed as breaking in the proposal; the ASCII results do not change.
