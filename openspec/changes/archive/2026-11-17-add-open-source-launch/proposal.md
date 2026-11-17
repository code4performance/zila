## Why

Zila Lang needs people to test it, use it, and extend it (README.md's "Designed with AI, directed by humans, and in need of volunteers" section says so directly), but the repository (`code4performance/zila` on GitHub) is still private, so no outside developer can see it, clone it, or open an issue or PR. CONTRIBUTING.md, GOVERNANCE.md and CODE_OF_CONDUCT.md already exist, but nothing yet prepares the repo for a stranger's first PR (no issue/PR templates, no CODEOWNERS, no SECURITY.md, no "good first issue" labels) or tells anyone the project exists. Going public without that groundwork risks a quiet, uneventful flip with no first contributors and no visibility.

## What Changes

- Flip the GitHub repository from private to public once the readiness checklist below is met (manual, one-time action tracked in tasks.md — not itself a spec requirement).
- Add the repository artifacts a public, contributor-facing project needs: a GitHub issue template set (bug report, feature/design-question), a pull request template, a `SECURITY.md`, a `CODEOWNERS` file, and a documented labeling scheme (including `good first issue`).
- Verify the CI workflow (`.github/workflows/ci.yml`) safely runs on pull requests from forks (no secrets exposed, appropriate trigger/permissions).
- Curate a starter set of `good first issue`-labeled issues from the items already listed in README.md's "Where Zila Lang is going" / open-gaps section, so a new contributor has something concrete to pick up on day one.
- Produce a public-launch marketing plan: target channels (e.g. Hacker News, Reddit r/ProgrammingLanguages, relevant Discord/Zulip communities, X/Twitter, dev.to/blog post), a launch announcement draft, a timeline, and success metrics (stars, forks, first external PRs/issues).
- Add a short "why contribute" / landing section to the docs site (`docs/index.html`) linking to CONTRIBUTING.md and the curated starter issues, so traffic from the marketing push has somewhere useful to land.

**BREAKING**: none — this is additive (new files, new docs section) plus an operational step (making the existing private repo public). No existing language behavior, API, or spec changes.

## Capabilities

### New Capabilities
- `contributor-onboarding`: the repository artifacts and process a public project needs so an outside developer can safely and successfully open their first issue or PR (templates, CODEOWNERS, SECURITY.md, fork-safe CI, labeled starter issues).
- `launch-marketing`: the plan and the supporting docs-site/README surface needed to announce the public repository to developer communities and point newcomers at something they can act on.

### Modified Capabilities
(none — `docs-site`, `continuous-integration`, and `brand-identity` are touched only at the implementation level, e.g. adding a new docs page or confirming an existing CI trigger; no requirement in their existing specs changes)

## Impact

- **Repository visibility**: `code4performance/zila` changes from private to public on GitHub (manual action, irreversible in the sense that history becomes visible — tracked explicitly as a task with a readiness gate).
- **New files**: `.github/ISSUE_TEMPLATE/*`, `.github/pull_request_template.md`, `SECURITY.md`, `CODEOWNERS`, a labels reference doc, a launch announcement draft, a docs site "contribute" section.
- **CI**: possible adjustments to `.github/workflows/ci.yml` triggers/permissions for fork PRs.
- **Docs site**: `docs/index.html` gains a short contributor-facing section/link.
- **No changes** to the compiler, standard library, debugger, or any existing spec's requirements.
