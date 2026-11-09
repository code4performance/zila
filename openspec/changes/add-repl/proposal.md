## Why

`zila eval` runs one snippet and exits, so there is still no way to sit down and try the language: type a line, see the result, type the next one that uses it. For a young language that needs volunteers to use it in practice, an interactive prompt is the lowest-friction way in. The earlier change `add-run-and-eval` left it out as a non-goal; it was the part of the request that mattered most.

## What Changes

- **`zila repl`** (also `zila -i`) starts an interactive session: a prompt reads an entry, runs it, prints what it printed, and keeps what it defined for the next entry.
- An entry is a statement, a `use`, or a declaration (`func`, `type`, ...), as in `zila eval`; a **single expression** (`1 + 2`, `name.len`) prints its value without writing `print`.
- Entries that open a block (`if`, `for`, `func`, `type`, ...) continue on the next lines (prompt `...`) until the block is complete.
- Variables, functions, types and imports stay for the rest of the session. An entry that fails to compile or aborts is reported and **discarded**: the session continues as if it had not been typed.
- Session commands: `:help`, `:reset` (forget everything), `:show` (print the session's code), `:quit` (or end of input, Ctrl-D / Ctrl-Z).
- Works with the terminal's own line editing and history, and with input that is not a terminal (a script piped in), where no prompts are written.
- **How it works, stated plainly:** Zila compiles to machine code, so there is no live interpreter to keep state in. Each entry builds and runs the whole session so far as one program, and only the output **after what was already shown** is printed. Earlier entries therefore run again with every entry: it is fast for small sessions, and entries with side effects (files, network, databases) repeat theirs. This is documented as the REPL's limit, with `:show` and `:reset` to manage it.

## Capabilities

### New Capabilities
- `repl`: the interactive session.

### Modified Capabilities

None.

## Impact

`src/repl.rs` (new), `src/cli.rs` (`repl` and `-i`), `src/main.rs` (usage, dispatch), `src/run.rs` (a way to build and run capturing output), `tests/repl.rs`, `docs/getting-started.html`, `README.md`, `CHANGELOG.md`. No change to the language, the compiler's output or the libraries.

Non-goals: keeping a live process between entries (needs an incremental compiler), tab completion, syntax highlighting, multi-line editing of earlier lines, a history file, remote or Jupyter-style kernels, and making side effects run once (see "How it works").
