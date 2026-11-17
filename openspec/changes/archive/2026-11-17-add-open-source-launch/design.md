## Context

See proposal.md - Why. Relevant current state:

- `code4performance/zila` is private on GitHub. `.github/workflows/ci.yml` already runs `pull_request` (not `pull_request_target`) and uses no secrets, so it is already safe to run on fork PRs without change — confirmed by reading the workflow file.
- CONTRIBUTING.md, GOVERNANCE.md, CODE_OF_CONDUCT.md, and two LICENSE files already exist. What's missing is the GitHub-native scaffolding (issue/PR templates, CODEOWNERS, SECURITY.md) and anything that announces the project outside the repo itself.
- The project already treats docs/branding consistency as testable spec requirements (see `brand-identity`, `docs-sync`), so this change follows the same pattern for contributor-onboarding and marketing readiness rather than leaving them as untracked tribal knowledge.
- Making the repository public is a single GitHub action with no staged rollout — it is effectively one-way (history, issues, and prior discussion become visible immediately and GitHub does not offer a clean re-privatization that undoes exposure).

## Goals / Non-Goals

**Goals:**
- Sequence the work so the repository is never public without the baseline a first-time contributor needs (templates, security contact, starter issues).
- Keep the marketing plan concrete enough to act on (named channels, named owner, named date) rather than a vague "post about it somewhere."
- Reuse existing project assets (the one description in `docs/assets/description.txt`, the existing CONTRIBUTING/GOVERNANCE docs, the open-gaps list in README.md) instead of creating parallel copies that can drift.

**Non-Goals:**
- Not designing a long-term community program (maintainer onboarding, triage rotation, roadmap governance beyond what GOVERNANCE.md already covers).
- Not building any paid/sponsored marketing (ads, PR firm, swag) — organic developer-community channels only.
- Not changing any compiler, stdlib, or tooling behavior — this change touches only repository metadata, docs, and process.
- Not reorganizing the broader docs site navigation/IA (`docs/*.html`) — this change adds one contributor-facing section to `docs/index.html` only; a wider reorganization of the documentation site is a separate concern, tracked in its own change.

## Decisions

- **Gate the "make public" action behind a checklist task, not a date.** The GitHub visibility flip is tracked as the last task in tasks.md, explicitly dependent on every contributor-onboarding artifact existing first. Alternative considered: pick a launch date and work backward; rejected because a date-driven plan tends to ship the flip even when the checklist isn't done, and the flip is effectively one-way.
- **Write one launch announcement, adapt per channel at post time.** A single `docs/launch/announcement.md` is the source of truth for the core message; Hacker News/Reddit/X/dev.to copies are derived from it when posting, not maintained as separate files. Alternative considered: a separate pre-written draft per channel; rejected as extra upkeep for content that's mostly identical and because tone-per-channel is better judged live than pre-scripted months ahead.
- **Starter issues come from README's existing gap list, not newly invented ones.** README.md already maintains a "Where Zila Lang is going" list; the `contributor-onboarding` spec requires picking from it rather than inventing fresh scope, keeping one list as the source of truth for "what's open."
- **No change to CI triggers.** Since `.github/workflows/ci.yml` already uses `pull_request` with no secrets in any step, the fork-safety requirement in the `contributor-onboarding` spec is satisfied by the current workflow as-is; tasks.md should confirm this rather than modify the file.

## Risks / Trade-offs

- [Flipping to public before the checklist is actually done, due to external pressure (e.g. a date already announced informally)] → Make the checklist itself the task that performs the flip, so there is no separate "remember to check first" step to skip.
- [Launch post underperforms or gets no traction on a given channel] → Low cost since channels are free/organic; mitigate by posting to more than one channel (Hacker News + a subreddit + a community chat) rather than betting on one.
- [Once public, history/issues become visible immediately, including any sensitive discussion in existing private issues] → Add an explicit task to review existing issues/PR history for anything not meant to be public before flipping visibility.
- [Good first issues go stale or get claimed with no update, discouraging future newcomers] → Out of scope for this change (ongoing maintenance), but noted in tasks.md as a follow-up for GOVERNANCE.md rather than invented as a new spec requirement here.
