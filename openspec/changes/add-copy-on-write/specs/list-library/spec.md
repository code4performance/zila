## MODIFIED Requirements

### Requirement: Ownership
All functions SHALL follow the existing list rules for element ownership: lists of texts, records and lists SHALL be retained and released correctly, so that building and dropping results leaves `live_objects()` unchanged. The functions SHALL NOT change their arguments, and a result SHALL be independent of its arguments: a later write to the result SHALL NOT change an argument and the reverse, whether the function built a new list or returned its argument unchanged (a returned argument is a copy-on-write copy, see `copy-on-write`).

#### Scenario: No leaks
- **WHEN** a program maps, filters, sorts and reverses a list of texts 500 times
- **THEN** `live_objects()` does not grow between iterations

#### Scenario: Result independent of argument
- **WHEN** a program takes `ys = list.slice(xs, 0, xs.len)` and pushes to `ys`
- **THEN** `xs` keeps its old length
