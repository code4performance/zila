## 1. The rename tool

- [ ] 1.1 `tools/rename-language.py`: name validation, derived forms (`<Name>`, `<name>`, `<NAME>`, command, extension ids, window class) and `--ext`, `--repo`, `--publisher` overrides; verify with `python tools/rename-language.py --self-test` (derivations, refusal of `9lives` and `two words`)
- [ ] 1.2 Scope rules (tracked text files; excluding the archive, `.git`, `target`, `Cargo.lock`, binary files and the script itself) and `--print-rules`; verify that a dry run on the repository lists only included files and none under `openspec/changes/archive/`
- [ ] 1.3 Dry run (default): planned `git mv` moves, per-file replacement counts and the changed lines, with no change to the tree; verify by hashing the working tree before and after
- [ ] 1.4 Apply mode: clean-tree check (`--force` to skip), checklist printout and `--yes`, `git mv` then content replacement, the manual-story report with line numbers, idempotence; verify on a copy that a second run reports nothing to do
- [ ] 1.5 Safety: never commits or pushes, refuses a bad name before touching anything, reports mixed-case forms it will not replace; verify with the self-test

## 2. Checks in the repository

- [ ] 2.1 `tests/rename.rs`: the leftover test (no old name in tracked files or file names outside the archive, except `tools/rename-allow.txt`), dormant before the rename and covered by a test of its matching rules; verify `cargo test --test rename` passes now
- [ ] 2.2 `tools/rename-allow.txt` (empty) and the name checklist in `CONTRIBUTING.md` (crate, GitHub, Marketplace and Open VSX publisher, domain, collisions, command length, file extension); verify the checklist text is what the tool prints
- [ ] 2.3 Mark the story-of-the-name places so the tool can find them (README, GOVERNANCE, the site pages); verify the dry run lists exactly those files as manual work

## 3. Rehearsal

- [ ] 3.1 Apply the tool with `Zorbo` on a throw-away copy; verify `cargo build` and the whole suite without `tests/async.rs` pass in the copy
- [ ] 3.2 In the copy, verify the documentation check, `zorbo-parse --fmt-check` on `std/` and `examples/`, the Node tests of the extension, the Linux tests under WSL, and the boot image build
- [ ] 3.3 In the copy, verify the leftover test fails only on the manual story places, and passes once they are rewritten; verify a `.vsix` manifest shows the new ids (or note that packaging is blocked, as in the extension change)
- [ ] 3.4 Record the rehearsal result and the list of manual places in this change, and delete the copy; verify `git status` of the real repository is unchanged

## 4. Housekeeping done now

- [ ] 4.1 Remove build and session debris from the working tree (`brasa-example.txt`, `lines-example.txt`, the empty `Barros/` folder tree); verify `git status` and `ls` show only project files (ignored build outputs stay ignored)
- [ ] 4.2 `openspec validate rename-language` passes and the whole suite (without `tests/async.rs`) passes; verify with `cargo test` and the validate command

## 5. The real rename (when the name is given)

- [ ] 5.1 Run the checklist for the chosen name and record the result; verify each item is answered
- [ ] 5.2 Dry run with the real name, review every changed line, then `--apply --yes` on a clean tree on a branch; verify `git diff --stat` and that `openspec/changes/archive/` is untouched
- [ ] 5.3 Rewrite the story of the name (README, GOVERNANCE, site pages, logo), add the "formerly known as" note and the allow-list entry; verify the leftover test passes
- [ ] 5.4 Update the extension (version `0.3.0`, ids, settings, README) and the repository metadata (`Cargo.toml` repository, links); verify the extension tests and the leftover test
- [ ] 5.5 Full verification: build for Windows and Linux, the whole suite without `tests/async.rs`, the docs check, `--fmt-check`, the boot image, `openspec validate --specs`; verify all pass
- [ ] 5.6 Outside the repository: rename the GitHub repository, the local folder and the Marketplace publisher, and copy the Claude Code memory folder to the new project key; verify the new URL opens, the old URL redirects, and a new session sees the saved memories
- [ ] 5.7 Archive this change; verify `openspec/specs/language-rename/` exists
