## Purpose

Keeps the Brasa documentation site accurate: changes to the language cannot be finished without updating the docs, and automated checks fail when the site and the source disagree.

## ADDED Requirements

### Requirement: Changes must update the documentation
Every change that alters the language syntax, compiler behavior, standard libraries, command-line interface or shipped examples SHALL include work items that update the affected documentation pages, and the change SHALL NOT be considered complete until those pages reflect the new behavior.

#### Scenario: A change alters user-visible behavior
- **WHEN** a change adds, removes or alters a language construct, a library function, a command-line flag or an example program
- **THEN** its task list contains a task that updates the affected `docs/` page and states how that update is verified

#### Scenario: A change has no user-visible effect
- **WHEN** a change only alters internals with no visible effect on language, libraries, CLI or examples
- **THEN** its tasks record that no documentation update is needed, with the reason

### Requirement: Embedded examples match the example files
Every example program shown in the documentation SHALL be identical, ignoring syntax-highlighting markup, to the corresponding file under `examples/`, and the project's test run SHALL fail when they differ.

#### Scenario: An example file is edited
- **WHEN** a file under `examples/` is changed and the documentation is not regenerated
- **THEN** the test run fails and names the example and the page that is out of date

#### Scenario: Examples are regenerated
- **WHEN** the documentation generator is run
- **THEN** every embedded example is rewritten from its source file and the drift check passes

### Requirement: Documented commands match the command line
The documentation SHALL describe every option the command-line tool accepts and SHALL NOT describe an option the tool rejects. The test run SHALL fail on any mismatch.

#### Scenario: A new flag is added to the tool
- **WHEN** the command-line tool gains an option that the getting-started page does not mention
- **THEN** the test run fails and names the undocumented option

#### Scenario: A documented flag is removed
- **WHEN** a flag shown in the documentation is no longer accepted by the tool
- **THEN** the test run fails and names the stale flag

### Requirement: Documented keywords match the language
The syntax reference SHALL mention every reserved word of the language and SHALL NOT present as a keyword any word that is no longer reserved. The test run SHALL fail on any mismatch.

#### Scenario: A keyword is added to the language
- **WHEN** a reserved word is added and the syntax page does not mention it
- **THEN** the test run fails and names the keyword

#### Scenario: A keyword is removed from the language
- **WHEN** a word that is no longer reserved is still shown in the syntax page as a keyword
- **THEN** the test run fails and names the word

### Requirement: Site integrity
Every page of the documentation SHALL share the same navigation, every internal link SHALL resolve to an existing page or anchor, and the site SHALL work when opened directly from disk without a server.

#### Scenario: A page is added without being linked
- **WHEN** a page is added under `docs/` and is missing from the shared navigation of the other pages
- **THEN** the test run fails and names the page

#### Scenario: A link points nowhere
- **WHEN** an internal link targets a page or anchor that does not exist
- **THEN** the test run fails and names the link and the page that contains it

### Requirement: Documentation covers the implemented language
The site SHALL document the features that are implemented, each with at least one short code sample: native compilation and its supported subset, the memory model, error handling with `or error` and `try`, enums with data and exhaustive `match`, tasks and channels, the `ui` library, and Brasa OS including how to build and run it. It SHALL state which documented features are not yet supported by the native compiler.

#### Scenario: Visitor looks for a feature
- **WHEN** a visitor opens the page for native compilation, concurrency or Brasa OS
- **THEN** the page explains the feature, shows a working sample or command, and lists its current limitations

#### Scenario: A feature is parsed but not compiled
- **WHEN** a documented construct is accepted by the parser but not supported by the native compiler
- **THEN** the page that documents it says so
