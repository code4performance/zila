## Purpose
Let people try Zila interactively: type an entry, see its result, and use what earlier entries defined.

## ADDED Requirements

### Requirement: An interactive session
`zila repl` and `zila -i` SHALL start a session that repeatedly reads an entry, runs it as `zila run` would, prints the output of that entry, and continues until `:quit` or the end of standard input. When standard input is a terminal the session SHALL write the prompt `zila> `; when it is not, it SHALL write no prompts or banner, so that its output is only what the entries printed.

#### Scenario: A statement
- **WHEN** the entry `print("hi")` is typed
- **THEN** `hi` is printed once and the prompt returns

#### Scenario: A script piped in
- **WHEN** `print(1)` and `print(2)` are piped to `zila repl` on separate lines
- **THEN** the output is `1` then `2` and the exit status is 0

### Requirement: Entries build on each other
Variables, functions, types and imports defined by an entry SHALL be available to all later entries until `:reset`.

#### Scenario: A variable
- **WHEN** `let x = 20` is entered and later `print(x + 1)`
- **THEN** `21` is printed

#### Scenario: A function
- **WHEN** a `func double(n: int) -> int ... end` is entered and later `print(double(21))`
- **THEN** `42` is printed

### Requirement: Output of earlier entries is not repeated
Only the output produced by the entry just typed SHALL be printed, even though earlier entries are run again with it.

#### Scenario: A second print
- **WHEN** `print("a")` and then `print("b")` are entered
- **THEN** the second entry shows `b` and not `a` again

### Requirement: An expression prints its value
An entry that is a single expression whose value can be printed SHALL print that value, as `print(<expression>)` would. An entry that is not such an expression (a statement, a call that returns nothing, an assignment) SHALL run as a statement.

#### Scenario: Arithmetic
- **WHEN** `1 + 2` is entered
- **THEN** `3` is printed

#### Scenario: A call that returns nothing
- **WHEN** `print("x")` is entered
- **THEN** `x` is printed once, not `x` and an error

### Requirement: Blocks continue on the next lines
An entry that is incomplete (an open block, an open parenthesis) SHALL make the session ask for more with the prompt `...> ` (when on a terminal) and run only when the entry is complete.

#### Scenario: An if block
- **WHEN** `if true`, `    print("yes")` and `end` are entered on three lines
- **THEN** `yes` is printed once, after the third line

### Requirement: A failed entry is discarded
An entry that does not parse, does not compile, or ends the program with an error or an abort SHALL be reported (the error text, naming the entry's line) and SHALL NOT become part of the session; the session SHALL continue with the state it had before.

#### Scenario: A type error
- **WHEN** `let n: int = "a"` is entered and then `print(1)`
- **THEN** an error is reported for the first, and `1` is printed for the second

#### Scenario: A name that was never defined
- **WHEN** `n` is entered after the failed `let n` above
- **THEN** an error says `n` is undefined

### Requirement: Session commands
`:help` SHALL list the commands, `:show` SHALL print the code of the session so far, `:reset` SHALL forget every entry, and `:quit` (and `:exit`) SHALL end the session with status 0. A line that starts with `:` and is not one of these SHALL be reported as unknown and not run.

#### Scenario: Reset
- **WHEN** `let x = 1`, `:reset` and `print(x)` are entered
- **THEN** the last entry reports that `x` is undefined

#### Scenario: Show
- **WHEN** `let x = 1` is entered and then `:show`
- **THEN** `let x = 1` is printed

### Requirement: The limit is stated
The documentation of the REPL SHALL state that each entry runs the whole session again and that side effects of earlier entries repeat.

#### Scenario: Documentation
- **WHEN** the getting-started page is read
- **THEN** it says that earlier entries run again with each new one
