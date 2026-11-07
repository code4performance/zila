## MODIFIED Requirements

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
