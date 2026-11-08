## ADDED Requirements

### Requirement: The tsql dialect
The `sqlserver` package SHALL declare the SQL dialect `tsql` with the profile: named placeholders `@pn`; single-quoted strings (`N'...'` accepted, no backslash escapes); `[x]` and double-quoted identifiers; `--` line comments and nesting `/* */` block comments; no dollar quoting; several statements per block allowed. A `GO` line SHALL be reported as an error saying it is a client tool command and not T-SQL. Connections returned by `sqlserver.open` SHALL have the dialect name `tsql`. A `sql tsql` statement SHALL run through the parameterized execution of the driver with its arguments as bound parameters.

#### Scenario: A block on a real server
- **WHEN** a `sql tsql` block with two holes is run on a SQL Server connection
- **THEN** the server receives the text with `@p1` and `@p2` and the two values as parameters, and the result is the same as the equivalent text statement

#### Scenario: Variables are not holes
- **WHEN** a block contains `declare @total int = 0; select @total = count(*) from t where id < {limit}`
- **THEN** `@total` is ordinary text and the only hole is `{limit}`, numbered `@p1`

#### Scenario: GO
- **WHEN** a block contains a line `GO`
- **THEN** compilation fails explaining that `GO` is not T-SQL
