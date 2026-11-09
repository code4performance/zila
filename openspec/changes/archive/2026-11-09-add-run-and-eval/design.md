## Context

`zila <file> --run` already builds a temporary native program (`src/run.rs`) and runs it, passing the terminal through and returning the exit status. The command line (`src/cli.rs`) is a flat list of flags with one mode per invocation; it already accepts one bare word, `build`, as a spelling of `--build`. A program needs `package main` and `func main` (see `examples/hello.zila`), and errors carry a line and column of the file that was parsed. See proposal.md for the motivation.

## Goals / Non-Goals

**Goals:**
- Running code inline or from a pipe costs one command and no boilerplate.
- Exactly the semantics of `--run`: same compiler, same libraries, same exit status. No second implementation.
- Error positions that refer to what the user typed.

**Non-Goals:**
- A REPL, program arguments, state between snippets, other targets (see proposal.md).
- A new notion of "script" in the language: wrapping is a tool feature, the compiler sees an ordinary program.

## Decisions

**Wrap in the tool, not in the compiler.** The snippet is turned into the text of an ordinary program and handed to the existing parse, check and `run_file` path. Alternative: a parser mode for "statements at the top level of a file". Rejected: it changes the grammar (`docs/zila.ebnf`), the formatter and the language server for a convenience of one command.

**Wrapping rule, by lines, no parsing.** A line that starts in column 0 with `use `, `func `, `async func `, `type `, `extern `, `embed `, `pub `, `package ` or `#[` starts a *declaration*; a one-line form (`use`, `embed`, `extern`, a `type` alias, `package`) ends on that line (an `extern` signature continues while its parentheses are open), the others run until the line that is exactly `end` in column 0 (nested blocks are indented, as the formatter writes them). Everything else is a *statement*. The program is: the `package main` line, the `use` lines, the declarations in order, then `func main() -> error`, the statements indented, and `return`. Alternative: try to parse the snippet as a file and fall back to a statement list on failure. Rejected: error messages for a snippet that is both would be unpredictable; the line rule is easy to state in the spec and to test. A snippet that declares `func main` skips wrapping entirely.

**Return type `error`.** `func main() -> error` is how programs written with `try` end (see the README example), so `try` works in a snippet. A snippet that fails a `try` exits with status 1 after printing the error, as a program does.

**Source map by line offset.** The wrapper records, for each line of the generated program, the snippet line it came from (or none for generated lines). Diagnostics are rewritten from generated to snippet positions before printing; the message `error: ... --> <snippet>:L:C` names the snippet with the label `<eval>` (or `<stdin>`). Column offsets differ only by the statement indentation (4 spaces), which is subtracted.

**Subcommands spelled like `build`.** `run` and `eval` become bare words that set the mode, as `build` does; `-e` is the short form of `eval`. The flag `--run` stays. `FLAGS` (checked against `docs/getting-started.html` by `tests/docs.rs`) gains `-e`; `run` and `eval` are documented in the same page and the usage text.

**Standard input.** `-` as the file means standard input. The tool reads it fully before compiling, so the program inherits an empty (end-of-input) standard input.

**Where the work lives.** A new module (`src/eval.rs`) holds the wrapper and the position mapping and has unit tests; `main.rs` only dispatches. `run.rs` is reused unchanged, which is what keeps behaviour identical.

## Risks / Trade-offs

- [A snippet whose declaration's closing `end` is indented, or a one-line declaration form the rule does not know] → the statement/declaration split is wrong and the compiler reports an error at a mapped position; the documented fix is to use `func main`. The rule is stated in the docs.
- [Shell quoting of multi-line snippets, especially on Windows `cmd`/PowerShell] → documented examples use `zila eval -` with a here-document or `run -` with a pipe for anything longer than a line.
- [The `-e` option name could clash with a future compiler flag] → the existing flags are all `--long`; `-o` is the only short one and `-e` follows it.
- [Temporary executables per invocation make `-e` slow for tight loops of one-liners] → inherent in a compiled language; the REPL non-goal records it.

## Open Questions

- Should a shebang line (`#!/usr/bin/env zila run`) be accepted and skipped? It does not change the specs here and can be added later.
