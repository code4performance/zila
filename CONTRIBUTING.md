# Contributing to Zila Lang

Thank you for wanting to help. Zila Lang is a young language and every kind of contribution matters: code, tests, documentation, examples, bug reports and ideas. This guide explains how to take part and what we expect, so your time goes into things that can be merged.

Before you start, please read:

- the [README](README.md) for what Zila Lang is and where it is going,
- the [Code of Conduct](CODE_OF_CONDUCT.md), which applies everywhere in the project,
- the [Project Charter](GOVERNANCE.md), which sets the design principles, the roles and how decisions are made.

---

## Ways to contribute

You do not have to write compiler code.

| You can | Start here |
|---|---|
| Report a bug | Open an issue with a **tiny program** that fails (see below) |
| Suggest a language feature | Open a **proposal** issue (see [Proposing a change](#proposing-a-change)) |
| Write an example program | Add a `.br` file under `examples/` and a short explanation |
| Improve the documentation | Edit `docs/*.html` (see [Keep the documentation in step](#2-keep-the-documentation-in-step)) |
| Write tests | Add cases under `tests/` |
| Write library code in Zila Lang | Add or extend a module under `std/` |
| Work on the compiler | Pick an item from the [roadmap](README.md#how-zila-will-evolve) and talk to us first |

Not sure where to begin? Look for issues labelled `good first issue`, or open an issue and say what you are interested in.

---

## Setting up

You need a recent stable **Rust** toolchain. Compiling Zila Lang programs needs nothing else (the compiler writes the executable itself); on Windows, Rust's default MSVC toolchain needs the Visual Studio C++ linker to build the compiler. **QEMU** is optional and only needed to boot Zila Lang OS.

```sh
git clone https://github.com/code4performance/zila
cd zila
cargo build
cargo test
```

Tests that need the Windows toolchain or QEMU skip themselves when those are not installed, so `cargo test` is meant to work on any machine with Rust. If a test you expect to run is skipped, check the message it prints.

A quick tour of the code is in the [README](README.md#what-is-in-the-repository).

---

## Reporting a bug

A good bug report lets someone else see the problem in a minute. Please include:

1. **The smallest Zila Lang program** that shows it (a few lines, not your whole project).
2. **What you ran**, for example `zila-parse bug.br -o bug.exe`.
3. **What you expected** and **what happened** (paste the exact message or output).
4. Your **Windows version** if the problem is in compiling.

If a program crashes or prints something impossible, say so plainly. Crashes in compiled programs are almost always compiler bugs, and they are the most valuable reports we get.

Security-sensitive problems (for example, code the compiler generates that could be exploited) should be reported privately; see [Reporting a concern](CODE_OF_CONDUCT.md#reporting-a-concern) for how to reach the maintainers.

---

## Proposing a change

How much process a change needs depends on its size.

| Kind of change | What to do |
|---|---|
| Typo, broken link, clearer wording, new test, small bug fix | Open a pull request directly. No prior issue needed. |
| New example, new standard-library function, better error message | Open an issue to say you are working on it, then a pull request. |
| **Anything that changes the language** (syntax, meaning, new keyword, new type) | Follow the **proposal process** below. |
| Anything that **breaks existing programs** | Proposal process, with a migration note, and a longer discussion. |

### The proposal process

1. **Open a proposal issue** titled `Proposal: <short name>`. Explain:
   - **Why**: the problem, with a small example program that is painful today.
   - **What**: the new behavior, with example code.
   - **Alternatives**: other ways to solve it, and why you prefer this one.
   - **Impact**: what it changes for existing programs, and what it costs the compiler.
2. **Discuss.** Proposals stay open for at least **7 days** (14 if they break existing programs) so people in other time zones can weigh in. Be ready to revise.
3. **Decision.** A maintainer closes the proposal as *accepted*, *rejected* or *deferred* and writes down the reason. Decisions are measured against the [design principles](GOVERNANCE.md#design-principles).
4. **Specify.** An accepted proposal becomes an [OpenSpec](openspec) change under `openspec/changes/<name>/`:
   - `proposal.md`: why, what, impact;
   - `specs/<capability>/spec.md`: requirements, each with scenarios written as *WHEN … THEN …* that a test can check;
   - `design.md`: how it will be built and why;
   - `tasks.md`: small tasks, each saying how to verify it.

   The `openspec` command-line tool helps with this, but the files are plain Markdown: if you prefer, write them by hand and a maintainer will help you shape them.
5. **Build.** Implement the tasks, with tests and documentation, in pull requests of reasonable size.
6. **Archive.** When every task is done, the change is archived and its spec is merged into `openspec/specs/`.

A pull request that changes the language without an accepted proposal will not be merged, however good the code is. It is nothing personal: it protects everyone's time and keeps the language coherent.

---

## The rules for code

These rules are what reviewers will check.

### 1. Tests are required

- Every bug fix comes with a test that fails without the fix.
- Every feature comes with tests for the behavior in its spec scenarios.
- Programs that must be compiled and run live in `tests/` (see `tests/async.rs` for the pattern: compile, run, check the output).
- `cargo test` must pass before you ask for a review.

### 2. Keep the documentation in step

The documentation site (`docs/`) is checked by tests, and some of it is generated.

- If you change a program in `examples/`, run `zila-parse --docs` to refresh the copies on the site. `cargo test` fails if they differ.
- If you add or remove a command-line option, document it in `docs/getting-started.html`.
- If you add or remove a keyword, mention it on `docs/syntax.html`.
- If you add a page, link it from the navigation of **every** page.
- If you change behavior, update the page that describes it, and its code samples must still compile.

### 3. Zero dependencies in the compiler

The compiler crate must have **no external dependencies**: it uses only Rust's standard library. This keeps the toolchain small, auditable and easy to build anywhere. Separate tools (for example a language server) may use dependencies if there is a good reason, in their own crate and with an accepted proposal.

### 4. Code that depends on the platform must say so

Anything that only works on Windows, or needs the Visual Studio tools or QEMU, must fail or skip with a clear message elsewhere, never with a crash. Keep platform-specific code in clearly named places.

### 5. Do not commit generated files

No `.exe`, no `.img` disk images, no `target/` output, no editor folders. `.gitignore` already blocks them; please do not work around it.

### 6. Keep `unsafe` small and explained

In Zila Lang code, an `unsafe` block should be as short as possible and have a comment saying why it is sound. In the Rust code of the compiler, prefer safe Rust.

### 7. Care about error messages

A compiler error is the language talking to a beginner. It should say **what is wrong**, **where**, and, when possible, **what to do**. A pull request that improves a message is always welcome.

### 8. Keep changes focused

One pull request, one purpose. A bug fix should not also reformat unrelated files. Small pull requests get reviewed faster.

### Style

- Rust: format with `cargo fmt`, and keep the code simple and readable over clever.
- Zila Lang: follow the style of the files under `examples/` and `std/` (four-space indent, `end`-closed blocks, comments that explain *why*).
- Commit messages: `component: short description`, in the imperative. For example `parser: reject async lambdas with a clear message` or `std: add fs.read`. Add a few lines of explanation when the reason is not obvious.

---

## Pull request checklist

Copy this into your pull request description.

```markdown
- [ ] It does one thing, and the description says what and why
- [ ] If it changes the language: it links an accepted proposal
- [ ] `cargo test` passes
- [ ] New or changed behavior has tests
- [ ] Documentation is updated (`zila-parse --docs` run if examples changed)
- [ ] No new dependencies in the compiler
- [ ] No generated files committed
- [ ] I can explain every line of this change
```

---

## Review

- A maintainer will respond, usually within a week. Maintainers are people with other jobs: a gentle reminder after a week is fine.
- Review is about the change, not the person. Expect questions and requests for changes; they are how the project stays coherent.
- You can disagree. Explain your reasoning; if we still differ, the [conflict-resolution rules](GOVERNANCE.md#conflicts) apply.
- Pull requests are merged by a maintainer once they have at least one approval and passing tests. Prefer a **rebase** over merge commits on your branch.

---

## Using AI tools

AI assistants are welcome as a tool, and parts of this project were built with one. The rule is simple: **you are responsible for what you submit.** Understand every line, run the tests yourself, and be ready to explain and defend the change in review. If a substantial part of a contribution was generated, say so in the pull request, so reviewers know where to look carefully.

---

## Licensing of contributions

The project is intended to be dual-licensed under **MIT** or **Apache-2.0** (see [GOVERNANCE.md](GOVERNANCE.md#licensing)). By submitting a contribution you confirm that you have the right to submit it, and that it may be distributed under those terms.

---

## Getting help

Open an issue with the label `question`, or comment on the proposal or pull request you are working on. There is no chat server yet: keeping conversations on GitHub means the answers stay searchable for the next person.

## Renaming the language

The language can be renamed in one step with `tools/rename-language.py`; see `openspec/specs/language-rename` for what it guarantees. Before using it, check that:

- the crate name is free on crates.io (or the crate is not published)
- the GitHub organization and repository name are free
- the extension publisher id is free on the VS Code Marketplace and on Open VSX
- a domain for the site is available
- no existing language, tool or command has the same name
- the command (name + '-parse') is short and easy to type
- the source file extension (.br) still fits, or --ext gives the new one

Then run it as a dry run first (`python tools/rename-language.py <Name> --display "<Display Name>"`), review every changed line, and apply it with `--apply --yes` on a clean tree. The lines that tell the story of the old name are listed and have to be written again by a person. `tools/rename-allow.txt` lists the lines that may keep the old name (a "formerly known as" note).
