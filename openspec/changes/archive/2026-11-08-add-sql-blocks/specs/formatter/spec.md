## ADDED Requirements

### Requirement: SQL blocks are kept as written
The lines of a `sql` block SHALL be kept as written, including the text of holes: the formatter SHALL move the block as a whole to the indentation of the statement that holds it and SHALL keep the relative indentation of its lines. Formatting SHALL not change the block's tokens, and a second pass SHALL change nothing.

#### Scenario: Relative indentation
- **WHEN** a `sql` block has a `select` line and a `where` line indented four spaces deeper, inside a function indented eight spaces wrong
- **THEN** after formatting the block sits one level inside its function and the `where` line is still four spaces deeper than `select`

#### Scenario: Text that looks like Zila Lang
- **WHEN** a block contains the line `end_date is null` or `-- end`
- **THEN** it is part of the block, and only a line that is just `end` closes it
