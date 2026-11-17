# launch-marketing Specification

## Purpose

Defines what must exist so the public repository launch is actually announced to developer communities, and so people who arrive from that announcement land somewhere that tells them what to do next.

## Requirements

### Requirement: Launch announcement content
The project SHALL maintain a single written launch announcement (e.g. `docs/launch/announcement.md` or equivalent) containing: what Zila Lang is (reusing the one description from `docs/assets/description.txt` per the `brand-identity` spec), why it is launching now, what kind of help is wanted, and a link to CONTRIBUTING.md and the starter issues.
Rationale: every channel (Hacker News, Reddit, X/Twitter, dev.to, Discord/Zulip) needs the same core message adapted to its format; a single source avoids drift between channels and reuses the project's one description instead of inventing a new one.

#### Scenario: Drafting a channel-specific post
- **WHEN** someone adapts the announcement for a specific channel (e.g. a Reddit post or a tweet thread)
- **THEN** the core claims (what it is, why now, what help is wanted) trace back to the maintained announcement document rather than being written from scratch

### Requirement: Channel and timeline plan
The project SHALL maintain a launch plan listing the target channels (at minimum: Hacker News, a relevant subreddit, and one real-time developer community such as Discord or Zulip), the intended posting order/timing, and who is responsible for each post.

#### Scenario: Preparing to flip the repository to public
- **WHEN** the readiness checklist in tasks.md is reviewed before making the repository public
- **THEN** the launch plan names the channels to post to and the order/timing of those posts

### Requirement: Success metrics are defined before launch
The launch plan SHALL define, before the repository is made public, the metrics that will be used to judge the launch (at minimum: GitHub stars, forks, and count of first-time external contributors opening an issue or PR) and the time window over which they will be checked (e.g. first 7 and first 30 days).

#### Scenario: Reviewing the launch a week later
- **WHEN** the 7-day checkpoint named in the launch plan is reached
- **THEN** the project can report the defined metrics (stars, forks, first-time external issue/PR authors) for that window because they were specified in advance

### Requirement: Docs site contributor landing section
The documentation site (`docs/index.html`) SHALL include a section aimed at prospective contributors, linking to CONTRIBUTING.md and to the current set of `good first issue`-labeled issues, so traffic arriving from the launch announcement has a concrete next action.

#### Scenario: A visitor arrives from the launch announcement
- **WHEN** a visitor follows a link from the launch announcement to the documentation site
- **THEN** the site has a section that tells them how to contribute and links to both CONTRIBUTING.md and the current starter issues
