## 1. Contributor onboarding scaffolding

- [x] 1.1 Add `.github/ISSUE_TEMPLATE/bug_report.md` and `.github/ISSUE_TEMPLATE/feature_or_question.md`; verify by opening "New issue" on the repo (or a local fork) and confirming both templates are offered instead of a blank issue
- [x] 1.2 Add `.github/pull_request_template.md` with the CONTRIBUTING.md checklist and an issue/proposal link prompt; verify by opening a draft PR and confirming the description is pre-filled
- [x] 1.3 Add `SECURITY.md` at the repo root naming a private reporting channel; verify it does not tell reporters to use the public issue tracker
- [x] 1.4 Add `CODEOWNERS` mapping key paths (e.g. `src/`, `openspec/`, `docs/`) to a reviewer; verify a test PR touching one of those paths gets an automatic review request
- [x] 1.5 Confirm `.github/workflows/ci.yml` runs on fork pull requests without exposing secrets (already true per design.md — no code change expected); verify by re-reading the workflow and confirming it uses `pull_request` (not `pull_request_target`) and no step references `secrets.*`
- [x] 1.6 Pick at least 5 items from README.md's "Where Zila Lang is going" list, open them as GitHub issues scoped to a single concrete task each, and label them `good first issue`; verify the label filter returns at least 5 issues — created 6 issues (#1-#6): Linux thread layer, TLS server-side, TLS close_notify, language server hover, VS Code extension packaging/smoke test, SQLite driver; verified with `gh issue list --label "good first issue"`
- [x] 1.7 Create or confirm the `good first issue` label exists with a clear description in the repo's label list; verify by viewing the repo's Issues → Labels page — the label already existed (GitHub default) with description "Good for newcomers", confirmed via `gh label list`

## 2. Launch marketing content

- [x] 2.1 Write `docs/launch/announcement.md` using the one description from `docs/assets/description.txt`, covering what Zila Lang is, why now, what help is wanted, and links to CONTRIBUTING.md and the starter issues; verify the claims match `docs/assets/description.txt` word-for-word where quoted
- [x] 2.2 Write a launch plan (channels, posting order/timing, owner per post) listing at minimum Hacker News, one subreddit (e.g. r/ProgrammingLanguages), and one real-time community (Discord or Zulip); verify every listed channel has a named owner and a relative posting time (e.g. "day 0", "day 1") — written to `docs/launch/plan.md`; every channel defaults to `@pbarrosnj` as owner (confirmed with the user as the project maintainer); which specific Discord/Zulip server is left as an explicit open question to resolve before Day 1, since Zila Lang has no chat server of its own yet
- [x] 2.3 Define success metrics and checkpoints (stars, forks, first-time external issue/PR authors; 7-day and 30-day windows) in the same launch plan document; verify the metrics and both windows are written down before task 3.1 is checked off — in `docs/launch/plan.md`'s "Success metrics and checkpoints" section, with a results table to fill in at each checkpoint
- [x] 2.4 Add a contributor-facing section to `docs/index.html` linking to CONTRIBUTING.md and the current `good first issue` list; verify by loading the page and confirming both links resolve — added a "Help build Zila Lang" section; verified with `cargo test --test docs internal_links_and_anchors_resolve`

## 3. Pre-flight review and going public

- [x] 3.1 Review existing (private) issues, PRs, and discussions for anything not meant to become public once visibility changes; verify by completing a pass over all open and closed issues/PRs and noting any that need redaction or closing first — reviewed via `gh issue list --state all`, `gh pr list --state all`, and discussions (disabled); only content is the 6 starter issues created in this session, nothing to redact. Also checked git history for credential-like filenames/commit messages; `tests/docker/.env` is a documented, localhost-only dev fixture (fine), and `docs/project-review-2026-11-09.md` (already in git history) contains candid internal critique but no secrets — user confirmed leaving it as-is
- [x] 3.2 Confirm every task in sections 1 and 2 is checked off before proceeding; verify by re-running `openspec status --change "open-source-public-launch"` equivalent review of this file — all 11 tasks in sections 1 and 2 are checked
- [ ] 3.3 Change the repository visibility from private to public on GitHub; verify with `gh repo view code4performance/zila --json isPrivate` reporting `false`
- [ ] 3.4 Post the launch announcement to each channel named in the launch plan, in the planned order; verify each post is live and linked back to the repository
- [ ] 3.5 At the 7-day and 30-day checkpoints, record the metrics defined in task 2.3 (stars, forks, first-time external contributors); verify by writing the observed numbers into the launch plan document

## 4. Documentation

- [x] 4.1 No compiler, standard library, CLI, or example behavior changes as part of this change, so no update to `docs/` reference pages or `examples/` is required beyond the new contributor-facing docs covered in tasks 2.1, 2.2, and 2.4
