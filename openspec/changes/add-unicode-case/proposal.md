## Why

`text.upper("ação")` gives `AçãO`: the letters outside ASCII stay as they were. For a language that is meant to be used in Portuguese, Spanish, French, German, Greek and Russian programs, that is a bug in practice: a user typed `ç` at a prompt and the program did not capitalise it. The spec said so on purpose ("ASCII letters only"), but the restriction protects nothing, and every program that needs real case mapping would have to carry its own tables.

## What Changes

- `text.upper(s)` and `text.lower(s)` change the case of the letters of the main alphabets that have a one-to-one case mapping: Latin (Latin-1 Supplement, Latin Extended-A, Latin Extended Additional, fullwidth), Greek (with accents), Cyrillic and Armenian. `upper("ação")` is `AÇÃO`, `lower("ÀÉÎÕÜ")` is `àéîõü`.
- Characters with no one-to-one mapping stay as they are: `ß` stays `ß` when upper-cased (the full mapping is `SS`, two characters), and letters of scripts outside the list (Georgian, Cherokee, Latin Extended-B, Greek Extended, ...) are unchanged. This is documented, not hidden.
- Pure ASCII text takes the same fast path as before and gives the same result.
- Text that is not valid UTF-8 is read as `for c in s` reads it (each bad byte as U+FFFD), so `upper` and `lower` never abort on it, unlike slicing.

## Capabilities

### New Capabilities

None.

### Modified Capabilities
- `text-library`: `upper` and `lower` map the case of Latin, Greek, Cyrillic and Armenian letters instead of ASCII only.

## Impact

`std/text.zila` (the mapping and the new `upper`/`lower`), `tests/text_char.rs` or a new `tests/text_case.rs` (a table test against Rust's own Unicode mapping), `docs/stdlib.html` (one table row), the header comment of `text.zila`, `CHANGELOG.md`. Code that relied on non-ASCII letters being left alone changes behaviour (**BREAKING** for that code, which is rare; the one place in the repository that lower-cases for protocol keys, `ai`/`mcp`, handles ASCII names).

Non-goals: full Unicode case mapping (multi-character results such as `ß` to `SS`, the final-sigma and Turkish-locale rules, titlecase), case-insensitive comparison and case folding, normalization, and a `capitalize` or `title` function.
