# Launch plan: making the repository public

This is the channel, timing, ownership and success-metrics plan for making `code4performance/zila` public, required by `add-open-source-launch` before the repository's visibility is flipped (task 3.3). It accompanies [`announcement.md`](announcement.md), the single source for the launch message.

**Owner of this plan and of every post below (default): Paulo Barros (`@pbarrosnj`)**, the project's maintainer, unless a specific post is reassigned below.

## Channels, order and timing

| Day | Channel | Owner | What to post |
|---|---|---|---|
| Day 0 | Hacker News ("Show HN") | `@pbarrosnj` | A "Show HN: Zila Lang" post adapting `announcement.md`'s core message to HN's plain-text, no-marketing style; link to the repository. |
| Day 0 | Zila Lang's own GitHub repository | `@pbarrosnj` | Make the repository public (task 3.3) immediately before the first external post, not after — a dead link to a private repo defeats every other post. |
| Day 1 | Reddit — r/ProgrammingLanguages | `@pbarrosnj` | A post adapted from `announcement.md`, framed for a language-design audience (what makes Zila Lang's design choices interesting), linking to the repository and to `good first issue`. |
| Day 1 | A real-time developer community (Discord or Zulip — see Open question below) | `@pbarrosnj` | A short message in a relevant server/channel (e.g. a programming-languages or Rust community, since the compiler is written in Rust) pointing at the repository and asking for testers, especially on Linux. |
| Day 2 | dev.to / blog post | `@pbarrosnj` | A longer-form adaptation of `announcement.md` with a code sample, for search visibility and for people who missed the Day 0/1 posts. |
| Day 2 | X/Twitter | `@pbarrosnj` | A short thread (3-4 posts) summarizing `announcement.md`, linking to the repository. |

Posting order matters: Hacker News and the repository visibility flip happen together on Day 0, since an HN post to a private repository is pointless. The other channels follow over the next two days so the launch doesn't look like a single burst with no follow-through.

**Open question:** which specific Discord or Zulip server to post in has not been decided yet — Zila Lang does not have its own chat server (see CONTRIBUTING.md's "Getting help" section). Pick one relevant existing community (e.g. a general Rust or programming-languages Discord) before Day 1, rather than inventing a new one for the launch.

## Success metrics and checkpoints

Defined here, before the repository is made public, per the `launch-marketing` spec:

| Metric | How to check |
|---|---|
| GitHub stars | `gh repo view code4performance/zila --json stargazerCount` |
| Forks | `gh repo view code4performance/zila --json forkCount` |
| First-time external contributors | `gh issue list --repo code4performance/zila --json author` and `gh pr list --repo code4performance/zila --json author`, counting authors who are not `@pbarrosnj` and had not previously interacted with the repository |

**Checkpoints:** 7 days after the repository goes public, and again at 30 days. At each checkpoint, record the three numbers above in the table below (task 3.5).

### Results

| Checkpoint | Date | Stars | Forks | First-time external contributors |
|---|---|---|---|---|
| 7-day | *(fill in when reached)* | | | |
| 30-day | *(fill in when reached)* | | | |
