## Context

`src/lsp.rs` is a small hand-written JSON-RPC server: full-text sync, parser errors, formatting. Name and type errors only appear when `zila` compiles to an executable. `add-typed-tree-emission` makes the checker produce a typed tree (types, call targets, bindings) before emission; that tree is the data source for everything here. The compiler writes PE files directly (`src/pe.rs`) with its own assembler, so there is no external toolchain to produce debug info for us. See proposal.md for motivation.

## Goals / Non-Goals

**Goals:**
- Semantic features answered from open-document text, in the editor's time budget.
- A debugger that works on programs `zila` produces, with no external debugger installed.

**Non-Goals:**
- Debugging optimized code perfectly (variables may be unavailable).
- Remote debugging, attach to a running process, hot reload, reverse debugging.
- Incremental compilation of the package (the checker is rerun; caching is a later optimization).
- Replacing the LSP client in the extension with a library (it stays dependency-free).

## Decisions

**1. Reuse the checker, not a second analyzer.** The server calls the same parse + check pipeline as the compiler, stopping before emission, over an overlay of open-document texts on top of the files on disk. Alternative: a separate lightweight indexer. Rejected: it would drift from the compiler and report different errors than `zila build`.

**2. Check is error-tolerant at the statement level.** Completion must work in half-typed code. The parser recovers at statement boundaries (skip to the next line at the same indent) and keeps a partial tree; the checker treats a recovered statement as of unknown type and suppresses errors that depend on it. Alternative: complete only from the last good parse. Rejected: it is stale exactly when the user is typing.

**3. Position index built from the typed tree.** Each name use and declaration records its source range and target; hover, definition, references and rename are lookups in this index, completion uses the scope at the cursor plus the receiver's type. The index is rebuilt per check and discarded.

**4. Debounce and cancel.** Checking runs after a short quiet period; a newer edit supersedes a running check (its results are dropped). Requests use the last finished index and return null if the document version moved on.

**5. Debug info is a sidecar file, not CodeView/PDB or DWARF.** `--debug` writes `program.zdbg` (line table: address range to file:line; per function: name, frame layout, locals with type descriptors and frame-relative or register locations). The executable is byte-identical, so debugging cannot change behavior. Alternatives: embed DWARF (gives lldb/gdb for free, but PE+DWARF tooling on Windows is weak and the type model of Zila Lang, lists, maps, shared references, would need custom descriptors anyway); PDB (closed format, large work). A DWARF emitter can be added later from the same tables without changing this design.

**6. Own debugger core over the OS API.** Windows: `CreateProcess` with `DEBUG_ONLY_THIS_PROCESS`, `WaitForDebugEvent`, `int3` (0xCC) breakpoints patched in and restored on step-over, trap flag for single steps. Linux: `ptrace`. `zila --dap` speaks DAP on stdio over this core. Step over/out use the line table and frame layout (return address from the frame chain; the compiler keeps a predictable frame). Alternative: drive lldb/gdb. Rejected: an external dependency and no knowledge of Zila Lang values.

**7. Value rendering from type descriptors.** The sidecar carries, per type, how to read it (scalar width and signedness, record field offsets, list/map/text layouts, enum tags, shared-reference indirection). The adapter reads process memory and renders; anything it cannot read is "unavailable". Layouts come from the code generator's own layout code so they cannot diverge.

**8. Extension changes are small.** The existing hand-written client gains the new capabilities in `initialize` and the providers come from the server; add `contributes.debuggers` (type `zila`, runtime executable `zila --dap`) and a launch configuration snippet. README states that the extension must be installed with the editor's own `--install-extension` (VS Code and forks such as Antigravity keep separate extension folders; copying files by hand left the extension unregistered).

## Risks / Trade-offs

- Checker cost on large packages makes typing laggy → debounce, cancel, and measure on the biggest example/package; fall back to syntax-only diagnostics above a time budget.
- Error-tolerant parsing can yield misleading follow-on errors → suppress errors depending on recovered nodes; test with truncated versions of every example.
- Line tables must stay correct as the code generator and optimizer change → a test compiles examples with `--debug`, checks every statement line maps to an address and that stepping a sample program visits lines in order.
- Debugger depends on frame layout stability → derive layouts from shared code, add tests for each value kind.
- Scope is large → delivered in stages (diagnostics, completion/hover, navigation, then debugger); each stage is usable alone.

## Migration Plan

Additive. Older clients ignore capabilities they do not use. `--debug` is opt-in. Rollback is removing the new capabilities from the handshake.

## Open Questions

- Should semantic diagnostics run on every keystroke or only on save for packages above some size? Assumed: every edit, debounced, with the time-budget fallback above.
- DWARF emission for use with lldb/gdb and other editors' stock debuggers: deferred, not part of this change.
