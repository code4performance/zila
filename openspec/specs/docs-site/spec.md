## Purpose

A static documentation website that explains what the Zila Lang language is, how its syntax works, and how to build and run `.br` programs, so a newcomer can learn the language without reading the grammar file or parser source directly.

## Requirements

### Requirement: Overview page
The site SHALL provide a landing page that states what Zila Lang is, its design goals, and who it is for, without requiring the reader to consult `zila.ebnf` or the Rust source.

#### Scenario: Visitor opens the site
- **WHEN** a visitor opens the site's home page
- **THEN** the page shows a plain-language description of Zila Lang's purpose and design goals, and links to the syntax guide, examples, and getting-started sections

### Requirement: Syntax reference
The site SHALL document Zila Lang's core syntax constructs — packages and `use` declarations, `record`/`union`/`enum`/`interface` types, functions (including generics and `ref` parameters), `var`/`let` bindings, control flow (`if`, `match`, `for`, `while`), error handling (`!` return types and `?` propagation), `defer`, string interpolation, attributes (`#[...]`), and `extern`/`asm` FFI blocks — each with at least one short code sample.
Documented syntax SHALL be consistent with the grammar in `zila.ebnf` at the time the page is written (no documented construct may contradict the grammar).

#### Scenario: Visitor looks up a language construct
- **WHEN** a visitor opens the syntax reference and looks for a specific construct (e.g., `match` expressions or error propagation with `?`)
- **THEN** the page shows a description of that construct and a code sample demonstrating it

#### Scenario: Documented construct matches the grammar
- **WHEN** a documented syntax construct is compared against `zila.ebnf`
- **THEN** the documented form is valid according to the grammar (no documented syntax is contradicted by the grammar)

### Requirement: Annotated examples
The site SHALL present annotated walkthroughs of the example programs shipped in `examples/` (at minimum `hello.br`, `rest_api.br`, and `types.br`), showing the source code alongside an explanation of what each program demonstrates.

#### Scenario: Visitor reviews an example
- **WHEN** a visitor opens an example walkthrough
- **THEN** the page shows the example's Zila Lang source code and a written explanation of the language features it demonstrates

### Requirement: Getting started guide
The site SHALL explain how to build the Zila Lang parser and run a `.br` file, including the `--ast`, `--tokens`, and `--run` command-line options. It SHALL describe `--run` as compiling the program to a temporary native executable for this machine, running it and passing its exit status through, and SHALL NOT describe an interpreter.

#### Scenario: Visitor wants to run a program
- **WHEN** a visitor opens the getting-started page
- **THEN** the page shows the commands to build the project and run a `.br` file, including what each of `--ast`, `--tokens`, and `--run` does, with `--run` described as compile and run

### Requirement: Self-contained static site
The site SHALL be viewable by opening its entry HTML file in a browser directly from the filesystem (no server, build step, or network access required), and all internal navigation links SHALL resolve to pages within the site.

#### Scenario: Visitor opens the site offline
- **WHEN** a visitor opens the site's entry HTML file from a local folder without an internet connection
- **THEN** all pages render with their styling and all internal links between pages work

#### Scenario: Visitor follows internal navigation
- **WHEN** a visitor clicks a navigation link to another section of the site
- **THEN** the browser loads the corresponding page within the site with no broken link
