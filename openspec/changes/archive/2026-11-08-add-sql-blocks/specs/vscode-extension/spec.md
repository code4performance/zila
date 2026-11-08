## ADDED Requirements

### Requirement: SQL inside sql blocks
The editor grammar SHALL highlight the lines of a `sql <dialect>` block as SQL (keywords, strings, comments, numbers) and the `{expr}` holes as Zila Lang expressions, and SHALL end the region at a line that is only `end`. Outside a block, an identifier `sql` SHALL not start the region.

#### Scenario: A block
- **WHEN** a file contains a `sql postgres` block with a `where id < {limit}` line
- **THEN** `select` and `where` are scoped as SQL keywords and `limit` as a Zila Lang expression

#### Scenario: A parameter named sql
- **WHEN** a line is `func run(sql: text)`
- **THEN** no SQL region starts
