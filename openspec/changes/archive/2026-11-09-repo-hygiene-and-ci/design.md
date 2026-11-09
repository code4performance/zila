## Decisions

**Roadmap by test, not by generator.** The idea was to generate the README list from `openspec/changes/`. The README's gaps are prose (they come from unchecked tasks and from areas with no change yet), so generating them would lose the explanation; and at the moment `openspec/changes/` is empty. The test checks the invariant that went wrong instead: a name in backticks that looks like a change id (`add-`, `remove-`, `restrict-`, `split-`, `rename-`, `use-`, `keep-`, `redesign-`, `language-`) must be an open change, and every open change must be named. When someone opens a change they must add it to the roadmap, and when they archive it the test makes them remove it.

**Tag at the CHANGELOG's own commit.** Tagging today's `main` as `0.1.0` would put the command rename, the `.zila` docs moves and the refactor under a version the CHANGELOG dated 2026-11-08. `f8529fa` is where the section was written, and its `Cargo.toml` says `0.1.0`. The next release will be `0.2.0` (the rename of the command is a breaking change while below 1.0).

**Allow on the item.** Rewriting index-parallel loops or splitting three recursive collectors only to satisfy a lint would be churn in code with strong tests and a byte-identical-output oracle (`tools/asm-snapshot.py`, `tests/determinism.rs`). Each `#[allow]` sits on the one item it concerns.

**The Word file.** `Zila.docx` was kept in step by the mechanical rename of `use-zila-file-extension`, which shows it was maintained by hand as a copy of the README. Two copies of the same text diverge; the README is the canonical one, so the copy goes.

**CI on Linux is unproven.** The workflow was written without a runner available. Some test targets may assume Windows (they skip themselves when the toolchain is missing, by convention); the first run on `ubuntu-latest` may show tests that need a skip guard. That is expected and is the point of having the job.
