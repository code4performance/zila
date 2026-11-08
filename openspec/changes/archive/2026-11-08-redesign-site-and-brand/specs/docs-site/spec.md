## ADDED Requirements

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
