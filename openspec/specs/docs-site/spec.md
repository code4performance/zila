## Purpose

A static documentation website that explains what the Zila Lang language is, how its syntax works, and how to build and run `.zila` programs, so a newcomer can learn the language without reading the grammar file or parser source directly.

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
The site SHALL present annotated walkthroughs of the example programs shipped in `examples/` (at minimum `hello.zila`, `rest_api.zila`, and `types.zila`), showing the source code alongside an explanation of what each program demonstrates.

#### Scenario: Visitor reviews an example
- **WHEN** a visitor opens an example walkthrough
- **THEN** the page shows the example's Zila Lang source code and a written explanation of the language features it demonstrates

### Requirement: Getting started guide
The site SHALL explain how to build the Zila Lang parser and run a `.zila` file, including the `--ast`, `--tokens`, and `--run` command-line options. It SHALL describe `--run` as compiling the program to a temporary native executable for this machine, running it and passing its exit status through, and SHALL NOT describe an interpreter.

#### Scenario: Visitor wants to run a program
- **WHEN** a visitor opens the getting-started page
- **THEN** the page shows the commands to build the project and run a `.zila` file, including what each of `--ast`, `--tokens`, and `--run` does, with `--run` described as compile and run

### Requirement: Self-contained static site
The site SHALL be viewable by opening its entry HTML file in a browser directly from the filesystem (no server, build step, or network access required), and all internal navigation links SHALL resolve to pages within the site.

#### Scenario: Visitor opens the site offline
- **WHEN** a visitor opens the site's entry HTML file from a local folder without an internet connection
- **THEN** all pages render with their styling and all internal links between pages work

#### Scenario: Visitor follows internal navigation
- **WHEN** a visitor clicks a navigation link to another section of the site
- **THEN** the browser loads the corresponding page within the site with no broken link

### Requirement: A minimal layout
The pages SHALL share one stylesheet with a single reading column no wider than 70 characters, a body text size of at least 16 px with a line height of at least 1.6, system fonts only (no font file or request to another site), one accent colour on a neutral palette, and light and dark themes chosen by the system setting. Code blocks SHALL have a thin border and no shadow, and the page SHALL look the same without JavaScript, which the pages SHALL NOT need.

#### Scenario: Nothing is fetched
- **WHEN** a page is loaded with the network disabled after the files are read from disk
- **THEN** it is complete: no stylesheet, font, script or image from another origin is referenced

#### Scenario: Dark system theme
- **WHEN** the system prefers a dark colour scheme
- **THEN** the page uses the dark palette and every text and background pair has a contrast ratio of at least 4.5

### Requirement: Navigation
Wide screens SHALL show the site navigation as a fixed side list with the current page marked (`aria-current="page"`); narrow screens (under 800 px) SHALL show it behind a "Menu" control that works without JavaScript (a checkbox toggled by its label, which keeps the navigation a single `<nav>` element). The navigation markup (`<nav class="site-nav">` with the same links in the same order on every page) SHALL be unchanged, and a skip link to the content SHALL be the first focusable element.

#### Scenario: The same links everywhere
- **WHEN** the navigation of any two pages is compared
- **THEN** the links are the same and in the same order

#### Scenario: Keyboard
- **WHEN** a page is used with the keyboard only
- **THEN** the first Tab reaches "Skip to content", and focus is always visible

### Requirement: Readable on a phone and in print
At 360 px wide no page SHALL scroll sideways except inside a code block, and printing a page SHALL hide the navigation and use black text on white.

#### Scenario: A phone
- **WHEN** a page is shown at 360 px
- **THEN** the page body has no horizontal scrollbar and a wide code block scrolls on its own
