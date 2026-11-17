## MODIFIED Requirements

### Requirement: Site integrity
Every page of the documentation SHALL share the same grouped navigation (same groups, same order, same links within each group), every internal link — including links from the examples index to its topic pages and between topic pages — SHALL resolve to an existing page or anchor, and the site SHALL work when opened directly from disk without a server.

#### Scenario: A page is added without being linked
- **WHEN** a page is added under `docs/` and is missing from the shared navigation of the other pages
- **THEN** the test run fails and names the page

#### Scenario: A link points nowhere
- **WHEN** an internal link targets a page or anchor that does not exist
- **THEN** the test run fails and names the link and the page that contains it

#### Scenario: Navigation groups drift between pages
- **WHEN** two pages are compared and their navigation groups, group order, or the links within a group differ
- **THEN** the test run fails and names the page and the group or link that differs

#### Scenario: An example is unreachable from the index
- **WHEN** an example walkthrough exists under a topic page but is not linked from the examples index (directly or through that topic page)
- **THEN** the test run fails and names the unreachable example
