## ADDED Requirements

### Requirement: Text typed at a terminal is UTF-8
On Windows, `fs.stdin()` SHALL make a console's input arrive as UTF-8, as the program's output is already written, so that accented letters and other non-ASCII characters typed at the prompt are valid text. When standard input is a pipe or a file its bytes SHALL be passed through unchanged.

#### Scenario: Accents at the prompt
- **WHEN** `ç` is typed at a Windows terminal whose input code page is CP850 and the program reads the line
- **THEN** the line is the text `ç` (two bytes of UTF-8) and operations such as `text.upper(line)` do not abort

#### Scenario: Piped UTF-8
- **WHEN** `ação` is piped in as UTF-8
- **THEN** `read_line()` returns `ação`
