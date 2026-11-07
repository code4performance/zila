## Purpose

Gives programs the everyday operations on text (searching, splitting, trimming, replacing, parsing numbers) so they do not have to be written by hand.

## Requirements

### Requirement: Searching
`use text` SHALL provide `contains(s, part)`, `starts_with(s, prefix)`, `ends_with(s, suffix)` and `index_of(s, part)`. `index_of` SHALL return the byte position of the first match, or `-1`. An empty `part` SHALL match at position 0.

#### Scenario: Found and missing
- **WHEN** a program evaluates `text.index_of("hello world", "o w")`, `text.index_of("abc", "z")` and `text.contains("abc", "")`
- **THEN** the results are `4`, `-1` and `true`

#### Scenario: Prefix and suffix
- **WHEN** a program evaluates `text.starts_with("report.txt", "rep")` and `text.ends_with("report.txt", ".md")`
- **THEN** the results are `true` and `false`

### Requirement: Splitting and joining
`split(s, sep)` SHALL return a list of the pieces between non-overlapping occurrences of a non-empty `sep`, keeping empty pieces; with no occurrence it SHALL return a list holding `s`. An empty `sep` SHALL abort the program with the message `split needs a separator`. `join(parts, sep)` SHALL put `sep` between the items. `lines(s)` SHALL split on `\n`, drop a trailing `\r` from each line and not produce a final empty line when `s` ends with a newline.

#### Scenario: Split
- **WHEN** a program splits `"a,b,,c"` on `","` and prints the length and the third piece
- **THEN** it prints `4` and an empty text

#### Scenario: Join round trip
- **WHEN** a program joins `["x", "y", "z"]` with `"-"`
- **THEN** the result is `x-y-z`

#### Scenario: Lines
- **WHEN** a program takes `text.lines("one\r\ntwo\nthree\n")`
- **THEN** it gets `["one", "two", "three"]`

#### Scenario: Empty separator
- **WHEN** a program splits on an empty separator
- **THEN** it aborts with `split needs a separator` and exits with 101

### Requirement: Trimming, replacing and repeating
`trim`, `trim_start` and `trim_end` SHALL remove spaces, tabs, carriage returns and newlines from the ends. `replace(s, from, to)` SHALL replace every non-overlapping occurrence of a non-empty `from`, scanning left to right; an empty `from` SHALL return `s` unchanged. `repeat(s, n)` SHALL return `s` `n` times, and an empty text for `n <= 0`.

#### Scenario: Trim
- **WHEN** a program trims `"  \t hi there \n"`
- **THEN** the result is `hi there`

#### Scenario: Replace
- **WHEN** a program evaluates `text.replace("aaaa", "aa", "b")`
- **THEN** the result is `bb`

#### Scenario: Repeat
- **WHEN** a program evaluates `text.repeat("ab", 3)` and `text.repeat("ab", 0)`
- **THEN** the results are `ababab` and an empty text

### Requirement: Case and reversal
`upper(s)` and `lower(s)` SHALL change ASCII letters only and leave every other byte as it is. `reverse(s)` SHALL reverse the bytes.

#### Scenario: ASCII case
- **WHEN** a program uppercases `"Straße 1"`
- **THEN** the result is `STRAßE 1` (the non-ASCII bytes are unchanged)

### Requirement: Padding
`pad_left(s, width, fill)` and `pad_right(s, width, fill)` SHALL add copies of the one-byte text `fill` until the text is `width` bytes long, and SHALL return `s` unchanged when it is already that long or longer.

#### Scenario: Number column
- **WHEN** a program evaluates `text.pad_left("7", 3, "0")` and `text.pad_right("ab", 4, ".")`
- **THEN** the results are `007` and `ab..`

### Requirement: Parsing numbers
`parse_int(s)` SHALL return the integer written in `s` (optional leading `-` or `+`, decimal digits, no surrounding spaces, no separators) as `int or error`. An empty text, a non-digit, or a value outside the `int` range SHALL give an error whose message names the text, such as `not a whole number: "12x"`. `parse_float(s)` SHALL accept an optional sign, digits with an optional fraction and an optional exponent (`1.5`, `-2e3`, `.5`, `5.`), plus `inf`, `-inf` and `NaN`, as `float or error`; the result SHALL be the float nearest to the written value.

#### Scenario: Valid and invalid integers
- **WHEN** a program parses `"42"`, `"-7"`, `"12x"`, `""` and `"9223372036854775808"`
- **THEN** the first two give `42` and `-7` and the others give errors

#### Scenario: Fallback with ??
- **WHEN** a program evaluates `text.parse_int("oops") ?? 0`
- **THEN** the result is `0`

#### Scenario: Floats
- **WHEN** a program parses `"1.5"`, `"-2e3"`, `"0.1"` and `"abc"`
- **THEN** the first three give `1.5`, `-2000.0` and `0.1` and the last gives an error

#### Scenario: Round trip with printing
- **WHEN** a program parses the printed text of `0.1 + 0.2` back with `parse_float`
- **THEN** it gets exactly the same float

### Requirement: No leaks
Every function SHALL release the texts and lists it creates and does not return.

#### Scenario: Repeated use
- **WHEN** a program calls `split`, `join`, `replace` and `trim` on varying texts 1000 times
- **THEN** `live_objects()` does not grow between iterations
