# contributor-onboarding Specification

## Purpose

Gives an outside developer, who has never seen the Zila Lang repository before, everything they need to safely and successfully open their first issue or pull request once the repository is public.

## Requirements

### Requirement: Issue templates
The repository SHALL provide GitHub issue templates under `.github/ISSUE_TEMPLATE/` covering at least a bug report and a feature/design-question, each prompting for the information CONTRIBUTING.md and GOVERNANCE.md already require (for a design question: which principle is at stake; for a bug: the smallest reproducing program).

#### Scenario: Opening a new issue offers a template
- **WHEN** a contributor opens a new issue on the public repository
- **THEN** GitHub presents a choice of the bug-report and feature/design-question templates instead of a blank issue

### Requirement: Pull request template
The repository SHALL provide a pull request template at `.github/pull_request_template.md` that prompts the author to confirm they followed CONTRIBUTING.md (including running the relevant tests) and to link the issue or proposal the PR addresses.

#### Scenario: Opening a new PR shows the checklist
- **WHEN** a contributor opens a new pull request against the repository
- **THEN** the PR description is pre-filled with the pull request template's checklist and linking prompt

### Requirement: Security reporting path
The repository SHALL provide a `SECURITY.md` at the repository root describing how to privately report a security vulnerability, separate from the public issue tracker.

#### Scenario: Someone looks for how to report a vulnerability
- **WHEN** a visitor opens `SECURITY.md` at the repository root
- **THEN** it names a private reporting channel (e.g. a security advisory or private email) and does not direct the reporter to open a public issue

### Requirement: Code ownership for review routing
The repository SHALL provide a `CODEOWNERS` file so pull requests touching a given area are automatically routed to a reviewer who can act on them.

#### Scenario: A PR changes compiler code
- **WHEN** a pull request modifies files under a path listed in `CODEOWNERS`
- **THEN** GitHub automatically requests review from the owner(s) mapped to that path

### Requirement: Fork pull requests run CI safely
The CI workflow SHALL run automated checks on pull requests opened from forks, and SHALL NOT expose repository secrets to code running from a fork's pull request.
Rationale: a public repository without working CI on fork PRs gives external contributors no automated feedback, and leaking secrets to untrusted fork code is a supply-chain risk.

#### Scenario: External contributor opens a PR from a fork
- **WHEN** a pull request is opened from a forked repository
- **THEN** the CI workflow runs the test suite on that PR and completes without any step having access to repository secrets

### Requirement: Starter issues for new contributors
The repository SHALL maintain at least five open issues labeled `good first issue`, each scoped to a single, concrete, already-identified gap (drawn from the open items already listed in README.md), small enough for someone unfamiliar with the codebase to attempt.

#### Scenario: A newcomer looks for something to work on
- **WHEN** a contributor filters the repository's issues by the `good first issue` label
- **THEN** at least five issues are returned, each describing a single concrete, scoped task
