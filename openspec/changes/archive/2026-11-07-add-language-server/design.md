## Context

The crate has no dependencies and already has a parser that returns a `Diag { line, col, msg }` and a formatter (`format_source`). LSP is JSON-RPC with `Content-Length` framing.

## Decisions

1. **No dependencies.** A ~150-line JSON value type with a reader (including `\u` escapes and surrogate pairs) and a writer; the crate stays buildable without a registry.
2. **Pure `handle` function.** `handle(message, documents, out, shutdown)` is separate from the I/O loop in `run`, so tests drive it message by message with an in-memory buffer; no process is started.
3. **Full sync.** `textDocumentSync = 1`: every change carries the whole text, which is simple and cheap for source files.
4. **Diagnostics from the parser only.** One error per document (the parser stops at the first one). Type errors need the code generator and a project model, which is a later step. The range runs from the error column to the end of that line; columns are converted to UTF-16 units as the protocol requires.
5. **Formatting as one edit.** The range covers the whole document; no edit is returned when the text already matches.
6. **Exit status.** `exit` after `shutdown` gives 0, otherwise 1, as the protocol says.

## Risks

- Only the first syntax error is shown.
- Documents are kept in memory until `didClose`.
