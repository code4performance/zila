## Purpose

Write SQL inside a Brasa program as raw lines, checked as far as the dialect's lexical rules allow at compile time, with every value passed as a bound parameter and with the dialect described by a declaration so that more databases can be added without changing the compiler.

## ADDED Requirements

### Requirement: The sql block
A `sql <dialect>` followed by the end of the line SHALL start a block of raw lines that ends at the first line consisting only of `end` (optionally followed by a comment). The block SHALL be an expression of type `db.Sql`. `sql` SHALL be read as a block start only when the next token on the line is an identifier and the line ends there; elsewhere `sql` SHALL remain an ordinary identifier. A block without its `end` SHALL be reported as unterminated, naming the line where it began.

#### Scenario: A block as a value
- **WHEN** `let q = sql postgres` is followed by the line `select 1` and a line `end`
- **THEN** `q` is a `db.Sql` for dialect `postgres` whose text is `select 1` and which has no arguments

#### Scenario: An identifier called sql
- **WHEN** a function has a parameter `sql: text` and calls `conn.exec(sql)`
- **THEN** it compiles exactly as before

#### Scenario: Missing end
- **WHEN** the source stops inside a `sql` block
- **THEN** the compiler reports that the `sql` block is unterminated and the line where it began

### Requirement: Holes are bound parameters
Inside a block, `{expr}` written in code (outside string literals, quoted identifiers, comments and dollar-quoted text of the dialect) SHALL be a hole. Each hole SHALL become the dialect's placeholder for its position (first hole is parameter 1) in the final text, and its value SHALL be passed as a bound argument converted by the static type of `expr`: integers to an integer argument, `float` and `f32` to a float, `bool`, `text`, `T?` to `NULL` when `nil` and otherwise by the type of `T`, `[]float` to a vector, and a `db.Arg` unchanged. `{{` and `}}` SHALL write a literal `{` and `}`. The value of a hole SHALL never be written into the SQL text. A hole whose type has no conversion SHALL be a compile error naming the type and the line.

#### Scenario: Placeholders per dialect
- **WHEN** the same two-hole query is written as `sql postgres` and as `sql tsql`
- **THEN** the texts use `$1`, `$2` and `@p1`, `@p2` respectively and both carry the same two arguments in order

#### Scenario: Injection text is data
- **WHEN** a hole holds the text `x'); drop table customers; --`
- **THEN** the statement text is unchanged and the value is sent as a parameter, so the table still exists

#### Scenario: A hole inside a string
- **WHEN** a block contains `where name = 'a{b}c'`
- **THEN** the braces are part of the string and there is no hole

#### Scenario: Optional values
- **WHEN** a hole has the type `text?` and holds `nil`
- **THEN** the argument is `NULL`

#### Scenario: Unsupported type
- **WHEN** a hole has a record type
- **THEN** compilation fails saying that the type cannot be a parameter, naming it

### Requirement: Dialect profiles
A dialect SHALL be declared by an attribute on a declaration of a package, stating its name and lexical profile: the placeholder style (positional `$n`, named `@pn`, colon `:n`, or question mark), the string quotes and whether a backslash escapes inside them, the identifier quotes (`"x"`, `[x]`, `` `x` ``), the line and block comment forms and whether block comments nest, whether dollar-quoted text exists, and whether one block may hold several statements. `sql <name>` SHALL use the profile of the package that declares `<name>` among the packages the file imports; a name found nowhere SHALL be an error that names the package to `use` when the name is known to a standard package. Two imported packages declaring the same dialect name SHALL be an error. The standard library SHALL declare `postgres` (in package `postgres`) and `tsql` (in package `sqlserver`).

#### Scenario: Known dialect, not imported
- **WHEN** a file writes `sql tsql` without `use sqlserver`
- **THEN** compilation fails saying that dialect `tsql` is provided by `sqlserver`, which must be imported

#### Scenario: A new dialect without compiler changes
- **WHEN** a package declares a dialect `sqlite` with `?` placeholders, double-quoted identifiers and `--` comments, and a file imports it and writes `sql sqlite`
- **THEN** the block compiles with `?` placeholders and the checks of that profile

#### Scenario: Unknown dialect
- **WHEN** a file writes `sql oracle` and no imported package declares it
- **THEN** the error says that no imported package declares the dialect `oracle`

### Requirement: Lexical checks at compile time
The compiler SHALL read each block with its dialect's profile and report, with the line inside the file: an unterminated string, quoted identifier, block comment or dollar quote; unbalanced parentheses; a hand-written placeholder of the dialect's style in code (the message says to use `{expr}`); a `;` followed by more code when the dialect allows one statement per block; an empty block. A trailing `;` SHALL be accepted and removed.

#### Scenario: Unterminated string
- **WHEN** a block contains `where name = 'abc`
- **THEN** compilation fails with the line of the opening quote

#### Scenario: PostgreSQL dollar quoting
- **WHEN** a `sql postgres` block contains `select $$it's {not a hole}$$`
- **THEN** the dollar-quoted text is kept as it is and has no hole

#### Scenario: Bracket identifiers
- **WHEN** a `sql tsql` block contains `select [order] from [dbo].[t]` and a `sql postgres` block contains `a[1]`
- **THEN** the first reads `[order]` as an identifier and the second reads `[1]` as ordinary text

#### Scenario: Two statements
- **WHEN** a `sql postgres` block contains `select 1; select 2`
- **THEN** compilation fails saying the dialect allows one statement per block, while the same text in `sql tsql` is accepted

#### Scenario: Hand-written placeholder
- **WHEN** a `sql postgres` block contains `where id = $1`
- **THEN** compilation fails and says to write the value as `{expr}`

### Requirement: Running a statement
A connection SHALL run a `db.Sql` as it runs text with arguments, with the same typed rows, transactions and errors. A statement whose dialect differs from the connection's SHALL not be sent: the call SHALL return an error of kind `InvalidInput` naming both dialects. A `db.Sql` SHALL be an ordinary value that can be stored, passed and run more than once.

#### Scenario: Same code, two dialects
- **WHEN** `top_customers(conn, q)` runs a `sql postgres` statement on a PostgreSQL connection and a `sql tsql` statement on a SQL Server connection over the same data
- **THEN** both return the same rows

#### Scenario: Wrong dialect for the connection
- **WHEN** a `sql tsql` statement is run on a PostgreSQL connection
- **THEN** the call returns an `InvalidInput` error that names `tsql` and `postgres`, and nothing is sent to the server

### Requirement: Blocks cost nothing at run time beyond the arguments
A block with no holes SHALL compile to a constant statement; a block with holes SHALL build its argument list at run time and nothing else. A program that never runs a block SHALL not contain driver code only because the block exists.

#### Scenario: No holes
- **WHEN** a block has no holes
- **THEN** the generated code builds no argument list and no text at run time
