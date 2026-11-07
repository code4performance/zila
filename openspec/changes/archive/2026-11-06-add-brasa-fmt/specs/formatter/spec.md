## ADDED Requirements

### Requirement: Formatting files
`brasa-parse --fmt file.br [more.br ...]` SHALL rewrite each file in the standard style and print the names of the files it changed. `brasa-parse --fmt-check file.br ...` SHALL change nothing, print the names of the files that are not in the standard style and exit with status 1 when there are any (0 otherwise). A file that does not parse SHALL be reported with its error and left untouched, and the exit status SHALL be 1.

#### Scenario: Rewriting
- **WHEN** a badly indented file is given to `--fmt`
- **THEN** the file is rewritten in the standard style and its name is printed; running `--fmt` again changes nothing

#### Scenario: Checking
- **WHEN** `--fmt-check` is given a formatted file and an unformatted one
- **THEN** only the unformatted file is listed, no file changes and the exit status is 1; for formatted files only the status is 0

#### Scenario: Broken file
- **WHEN** a file with a syntax error is given to `--fmt`
- **THEN** the error is reported with its line, the file is not modified and the exit status is 1

### Requirement: Indentation
Each level of nesting SHALL be indented by four spaces. The body of `func` (declarations and lambdas), `if`/`else if`/`else`, `while`, `for`, `match`, `unsafe`, `asm`, `record`, `enum` and `interface` SHALL be one level deeper than the line that opens it; the closing `end` and each `else` SHALL be at the level of the line that opens the block. A line that continues an expression (it is inside an unclosed `(`, `[` or `{`, or the previous line ends with a binary operator or a comma inside brackets) SHALL be one level deeper than the line it continues. Function types such as `func(int) -> bool` and `extern` declarations SHALL NOT open a block. Tabs SHALL become spaces.

#### Scenario: Nested blocks
- **WHEN** a function has an `if` with an `else` inside a `while`
- **THEN** each body is four spaces deeper than its keyword, and `end` and `else` line up with their `if`, `while` and `func`

#### Scenario: Function types and lambdas
- **WHEN** a record field has the type `func(int) -> bool` and a call passes `func(a, b) a < b end`
- **THEN** the field does not indent the lines after it, and the one-line lambda does not change the indentation

#### Scenario: Continuation lines
- **WHEN** a call's arguments are split over several lines
- **THEN** the continued lines are one level deeper than the first and the closing bracket line is back at the level of the first

### Requirement: Spacing and blank lines
Within a line the formatter SHALL remove spaces before `,` `)` and `]` and after `(` and `[`, put exactly one space after each `,`, and leave the other spacing as written (runs of spaces used to align columns are kept; tabs between tokens become spaces). It SHALL remove trailing spaces, SHALL keep at most one blank line in a row, SHALL remove blank lines at the start and end of a block and of the file, and SHALL end the file with one newline. The file's line ending (LF or CRLF) SHALL be kept.

#### Scenario: Commas and brackets
- **WHEN** a line is written `foo( a ,b )`
- **THEN** it becomes `foo(a, b)`

#### Scenario: Blank lines
- **WHEN** a file has three blank lines between two functions and an empty line right after `func main()`
- **THEN** one blank line remains between the functions and none after `func main()`

### Requirement: Comments are kept
Every comment SHALL be kept with its text. A comment alone on its line SHALL take the indentation of the next code line, except that a comment right before a closing `end` SHALL stay inside the block (one level deeper than the `end`); a comment after code SHALL be separated from it by at least two spaces (a wider gap used for alignment is kept); the inside of block comments SHALL be left as written.

#### Scenario: Comments
- **WHEN** a file has comments on their own lines inside a function and after statements
- **THEN** they are all still there, aligned with the code that follows or at least two spaces after the code

### Requirement: Text and assembly are kept
The text of every token, including number spellings, string escapes and interpolations, SHALL be kept exactly. The lines of an `asm` block SHALL be indented one level deeper than `asm`, keeping the relative indentation they had (the least indented line sits at that level).

#### Scenario: Literals
- **WHEN** a file has `0xFF`, `"a\tb {x}"` and `1_000`, and a line with an interpolation is indented differently by the formatter
- **THEN** the literals come out unchanged

#### Scenario: Assembly
- **WHEN** an `asm` block has a label at one indentation and its instructions four spaces deeper
- **THEN** after formatting the label is one level inside `asm` and the instructions one level deeper than the label

### Requirement: The program never changes
The formatter SHALL write a file only when the sequence of tokens of the result equals the sequence of tokens of the original, and SHALL otherwise report an error and leave the file untouched. Formatting an already formatted file SHALL give the same text (idempotence).

#### Scenario: Tokens and idempotence
- **WHEN** every `.br` file of `std/`, `examples/` and `os/` is formatted
- **THEN** the tokens are equal before and after, a second pass changes nothing, and after the repository has been formatted `--fmt-check` lists none of them

### Requirement: The options are documented
The getting-started page SHALL describe `--fmt` and `--fmt-check`.

#### Scenario: Docs check
- **WHEN** the documentation test compares the options the tool accepts with the options the docs describe
- **THEN** both new options are described
