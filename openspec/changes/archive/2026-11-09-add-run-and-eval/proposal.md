## Why

Today the only way to run Zila code is `zila file.zila --run`: the code must be in a file, with `package main` and a `func main`, and the verb is a flag after the file name. For trying an idea, a one-liner in a shell script, or a pipe (`echo ... | zila run -`), that is too much ceremony, and it is the first thing a volunteer who wants to test the language in practice meets (see the README: "use it in practice").

## What Changes

- **`zila run <file> [--no-opt]`**: the same as `zila <file> --run`, as a subcommand that reads naturally. The flag form keeps working.
- **`zila eval "<code>"`** and **`zila -e "<code>"`**: run code given on the command line, with no file and none of the boilerplate. A snippet is a list of statements (and, optionally, `use` lines and declarations such as `func` and `type`); the tool wraps the statements in `package main` and `func main() -> error`, so `try` works and a failed `try` prints the error and exits with status 1.
- **`zila run -`** and **`zila eval -`**: read the program from standard input (a whole file, or a snippet), so it works in pipes and here-documents.
- A snippet that already defines `func main` is taken as a complete program (a `package main` line is added when missing).
- Everything else is the existing `--run` behaviour: it builds a temporary native program for this machine, runs it with the terminal's input and output, deletes it and returns its exit status; build errors run nothing and exit with 1.
- Errors in a snippet report line and column **relative to the snippet** (line 1 is the first line the user wrote), not to the wrapped program.

## Capabilities

### New Capabilities
- `eval-command`: running Zila code given on the command line or on standard input, and the `run` subcommand.

### Modified Capabilities
- `run-command`: `--run` gains the `zila run <file>` spelling with the same requirements.

## Impact

`src/cli.rs` (new modes and options, `FLAGS`), `src/main.rs` (usage text, dispatch), a new module that wraps snippets, `src/run.rs` (reused unchanged), `tests/run.rs` and a new `tests/eval.rs`, `docs/getting-started.html` (the flag list; the docs test checks flags and keywords), `README.md` ("Getting started"), `CHANGELOG.md`. No change to the language, to the compiler's output or to any executable it writes.

Non-goals: an interactive REPL (a compiled language would rebuild the whole program on every line; left for a later change), passing command-line arguments to the program, keeping state between snippets, running for another target, and a shebang (`#!`) line (can be a follow-up).
