## MODIFIED Requirements

### Requirement: Case and reversal
`upper(s)` and `lower(s)` SHALL change the case of every letter that has a one-to-one case mapping in these blocks: Latin-1 Supplement, Latin Extended-A, Latin Extended Additional, fullwidth Latin, Greek (with accents), Cyrillic and Armenian, and SHALL leave every other character as it is, including letters whose full mapping has several characters (`ß`) and letters of other scripts. For ASCII-only text the result SHALL be the same as an ASCII-only mapping. The results SHALL be valid UTF-8, and the functions SHALL NOT abort on text that is not valid UTF-8 (each bad byte is read as U+FFFD, as `for c in s` reads it). `reverse(s)` SHALL reverse the characters (code points), so the result is valid UTF-8 and `reverse(reverse(s)) == s`.

#### Scenario: ASCII case
- **WHEN** a program uppercases `"hello"` and lowercases `"HELLO"`
- **THEN** the results are `HELLO` and `hello`

#### Scenario: Portuguese
- **WHEN** a program uppercases `"ação"` and lowercases `"ÀÉÎÕÜ"`
- **THEN** the results are `AÇÃO` and `àéîõü`

#### Scenario: Other alphabets
- **WHEN** a program uppercases `"привет"` and `"αβγ ά"`, and lowercases `"ΈΛΛΑΣ"`
- **THEN** the results are `ПРИВЕТ`, `ΑΒΓ Ά` and `έλλασ`

#### Scenario: No one-to-one mapping
- **WHEN** a program uppercases `"Straße 1"`
- **THEN** the result is `STRAßE 1` (`ß` has no single-character capital)

#### Scenario: Agreement with Unicode
- **WHEN** every character of the supported blocks is mapped
- **THEN** the result equals the Unicode simple case mapping wherever that mapping is a single character

#### Scenario: Reversing Portuguese
- **WHEN** a program reverses `"ação"`
- **THEN** the result is `oãça`
