## Context

`a < b > (c)` is valid Brasa (two comparisons are rejected elsewhere, but `a < b, c > (d)` inside an argument list is not a call). The parser has no symbol table, so it cannot tell a generic function from a variable.

## Decisions

1. **Adjacency rule.** Type arguments are recognised only for a path callee (`name` or `a.b`), when `<` touches the previous token and `(` touches the closing `>`. The formatter writes spaces around binary operators, so formatted code never hits the ambiguity. `a<b, c>(d)` without spaces is read as a generic call (documented trade-off, as in C#).
2. **Speculative parse with a small undo.** The parser tries `< type {, type} >` and restores the position and the (at most 64) tokens it may have split (`>>`) when it does not end in `(`.
3. **Explicit first, inference second.** `call_generic` seeds its binding map with the explicit types (resolved in the caller's module) and infers only what is missing, so partial lists work.

## Risks

- Unspaced comparisons that look like a generic call change meaning. Mitigated by the adjacency rule and by the formatter.
