## MODIFIED Requirements

### Requirement: Navigation
Wide screens SHALL show the site navigation as a fixed side list with the current page marked (`aria-current="page"`); narrow screens (under 800 px) SHALL show it behind a "Menu" control that works without JavaScript (a checkbox toggled by its label, which keeps the navigation a single `<nav>` element). The navigation SHALL be organized into labeled groups (e.g. Language, Standard Library, Runtime & Native, Tooling, Zila Lang OS) within one `<nav class="site-nav">` element; the set of links, their grouping, their order within each group, and the group order SHALL be identical on every page. A skip link to the content SHALL be the first focusable element.

#### Scenario: The same links everywhere
- **WHEN** the navigation of any two pages is compared
- **THEN** the same groups appear in the same order, and within each group the same links appear in the same order

#### Scenario: Groups are labeled
- **WHEN** the site navigation is read
- **THEN** each group of links has a visible, non-link heading naming the group

#### Scenario: Keyboard
- **WHEN** a page is used with the keyboard only
- **THEN** the first Tab reaches "Skip to content", and focus is always visible

### Requirement: Annotated examples
The site SHALL present annotated walkthroughs of the example programs shipped in `examples/` (at minimum `hello.zila`, `rest_api.zila`, and `types.zila`), showing the source code alongside an explanation of what each program demonstrates. These walkthroughs SHALL be organized as a short examples index page linking to one or more topic pages grouping related examples (e.g. language basics, standard library, native/concurrency, web/AI), each topic page listing the examples it contains in an "On this page" summary; every example walkthrough the site shows today SHALL remain reachable from the index after the reorganization.

#### Scenario: Visitor reviews an example
- **WHEN** a visitor opens an example walkthrough
- **THEN** the page shows the example's Zila Lang source code and a written explanation of the language features it demonstrates

#### Scenario: Visitor browses examples by topic
- **WHEN** a visitor opens the examples index page
- **THEN** it links to each topic page, and each topic page's own "On this page" summary lists the examples grouped there

#### Scenario: An existing example is still reachable
- **WHEN** an example that was shown before the reorganization is looked for after it
- **THEN** it is reachable by following a link from the examples index, through at most one topic page
