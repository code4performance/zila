## Context

`src/eval.rs` turns a snippet into a program (declarations and `use` lines hoisted, statements in `func main() -> error`) and `src/run.rs` builds and runs a program for the host, inheriting the terminal. A parse error in a wrapped snippet that points at generated text (a position with no snippet line) or says "end of file" means the snippet is incomplete. A bare expression such as `1 + 2` is accepted as a statement and prints nothing. See proposal.md.

## Goals / Non-Goals

**Goals:** a prompt that feels interactive for small programs; state across entries; no new dependencies; behaves well when piped.

**Non-Goals:** a live process, completion, history file, running side effects once (see proposal.md).

## Decisions

**Replay, not a live interpreter.** The session is the text of the accepted entries. To run entry *n* the REPL wraps `entries + new` with `eval::wrap` (which already hoists declarations and keeps statement order), builds and runs it, captures standard output, and prints the part after the output length already shown. Alternatives: (a) a bytecode or tree interpreter inside the tool, rejected because the project removed its interpreter on purpose and a second implementation of the language would drift (spec `run-command`); (b) incremental native compilation keeping one process alive, rejected as far larger than this change and tied to the unfinished typed pipeline. Replay costs one compile per entry (about 0.2 s here) and repeats side effects; that is the limit the documentation states, with `:show` and `:reset` as the way out.

**Output delta.** After a successful run the REPL remembers how many bytes of standard output the whole session produced; the next run prints only bytes beyond that. If the new output is shorter than that (a non-deterministic program) everything is printed. Standard error is passed through whole.

**Expression entries.** An entry that is one line, starts with no statement keyword (`let`, `var`, `if`, `for`, `while`, `match`, `return`, `defer`, `spawn`, `use`, `func`, `type`, `extern`, `embed`, `pub`, `async`, `comptime`, `break`, `continue`, `unsafe`, `asm`, `#[`) and has no assignment operator is first tried as `print(<entry>)`. If that does not compile it is tried as a plain statement, so `print("x")`, a call returning nothing, still prints once. The fallback costs a second build only on that failure.

**Failed entries are dropped.** The new entry is appended to the session only after a run that built and exited with status 0. A build error, a runtime abort, or an error returned from `main` (the program prints `error: ...` itself) leaves the session unchanged. Errors name the entry's own line (`<repl>:N`) by reusing the line map of `eval::wrap` over the entry alone.

**Completeness check without compiling.** The entry is wrapped and parsed. A parse error whose position is generated text, or whose message ends in "end of file", means incomplete; any other parse error is a real error. Only a complete entry is built.

**Terminal or not.** `std::io::IsTerminal` on standard input decides whether prompts are written. Line editing and history are the terminal's own (the Windows console and Unix cooked mode): no line-editing library, keeping the project's no-dependencies rule.

**Spelling.** `repl` is a bare word like `run`, `eval` and `build`; `-i` is added to `FLAGS` (checked against `docs/getting-started.html`).

**Running.** `run.rs` gains a way to build and run capturing standard output and error and returning the status (the child's standard input is the terminal's, as for `zila run`), sharing the build step with `run_file`.

## Risks / Trade-offs

- [Side effects repeat on every entry: a `fs.write`, a database insert, a `sleep`] → stated up front in the docs and `:help`; `:reset` clears; a later change can add a mode that runs entries once with a persistent process.
- [A long session makes every entry slower (compile and run of everything)] → `:show` to inspect; `:reset` to restart; typical sessions are a few dozen lines.
- [Time-dependent or random output differs between replays, so the delta slice is wrong] → the prefix check prints everything when the output got shorter; otherwise the new tail is shown; documented.
- [A `let` of an existing name fails to compile] → reported like any error; the user picks another name or `:reset`.
- [Reading standard input from the program conflicts with the REPL reading its own commands] → the REPL consumes standard input; programs that read it see what is left, as with `zila eval -`. Documented.

## Open Questions

- Whether `zila` with no arguments on a terminal should start the REPL (today it prints the usage and exits with 2). Left as is; can be changed without touching the spec.
