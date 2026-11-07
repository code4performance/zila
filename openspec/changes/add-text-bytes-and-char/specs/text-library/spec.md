## ADDED Requirements

### Requirement: Characters
`use text` SHALL provide `chars(s) -> []char`, the characters of `s` in order, and `from_chars(cs: []char) -> text`, their concatenation, so that `from_chars(chars(s)) == s` for every valid `s`.

#### Scenario: Round trip
- **WHEN** a program evaluates `text.chars("ação").len` and `text.from_chars(text.chars("ação"))`
- **THEN** the results are `4` and `ação`

## MODIFIED Requirements

### Requirement: Case and reversal
`upper(s)` and `lower(s)` SHALL change ASCII letters only and leave every other character as it is. `reverse(s)` SHALL reverse the characters (code points), so the result is valid UTF-8 and `reverse(reverse(s)) == s`.

#### Scenario: ASCII case
- **WHEN** a program uppercases `"Straße 1"`
- **THEN** the result is `STRAßE 1` (the non-ASCII characters are unchanged)

#### Scenario: Reversing Portuguese
- **WHEN** a program reverses `"ação"`
- **THEN** the result is `oãça`

### Requirement: Padding
`pad_left(s, width, fill)` and `pad_right(s, width, fill)` SHALL add copies of the one-character text `fill` until the text is `width` characters long (counted as `char_count`, not `len`), and SHALL return `s` unchanged when it is already that long or longer. A `fill` that is not exactly one character SHALL abort with a message saying so.

#### Scenario: Number column
- **WHEN** a program evaluates `text.pad_left("7", 3, "0")` and `text.pad_right("ab", 4, ".")`
- **THEN** the results are `007` and `ab..`

#### Scenario: Accented text
- **WHEN** a program evaluates `text.pad_right("ação", 6, ".")`
- **THEN** the result is `ação..`
