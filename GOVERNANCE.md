# Zila Lang Project Charter

This charter is the statute of the Zila Lang project: what it stands for, who decides what, and the rules everyone who takes part agrees to follow. It is short on purpose. For the practical steps of sending a contribution, see [CONTRIBUTING.md](CONTRIBUTING.md). For how people must treat each other, see the [Code of Conduct](CODE_OF_CONDUCT.md).

## Contents

1. [Purpose](#purpose)
2. [Design principles](#design-principles)
3. [Roles](#roles)
4. [How decisions are made](#how-decisions-are-made)
5. [The life of a proposal](#the-life-of-a-proposal)
6. [Compatibility and versions](#compatibility-and-versions)
7. [Quality bar](#quality-bar)
8. [Conflicts](#conflicts)
9. [Licensing](#licensing)
10. [How governance will grow](#how-governance-will-grow)
11. [Amending this charter](#amending-this-charter)

---

## Purpose

Zila Lang is a compiled programming language that aims to be readable enough for a beginner and close enough to the machine to write an operating system. The project exists to build that language, its compiler, its standard library and its documentation, in the open, with a community.

The project does **not** exist to add every feature that is requested. A small, coherent language is worth more than a large, confusing one.

---

## Design principles

These principles are the tie-breakers. When two reasonable choices compete, the one that fits the principles better wins, and decisions record which principle they relied on.

1. **Readable first.** Code is read far more than it is written. A beginner should be able to read a Zila Lang program from top to bottom.
2. **Explicit over implicit.** If something important happens (a copy, a pointer, a failure, an unsafe operation, a point where a task may give up the processor), it is visible in the code.
3. **Simple over clever.** Prefer a plain construct to a syntactic trick. If a feature needs a long chapter to explain, it probably does not belong.
4. **Errors are values.** There are no exceptions. A function that can fail says so in its type, and propagation is visible (`try`).
5. **No implicit conversions.** Numbers of different sizes do not silently convert; text and raw C strings do not silently convert. The programmer decides.
6. **Unsafe must be loud.** Anything that can cause undefined behavior sits inside an `unsafe` block, which should be small and explained.
7. **Honest costs.** No hidden allocation or hidden runtime. Memory management is predictable (reference counting, with `drop` for resources).
8. **C is a first-class neighbour.** Calling C, being called from C, and running with no operating system are core uses, not afterthoughts.
9. **A lean, readable toolchain.** The compiler has no external dependencies. The standard library is written in Zila Lang wherever possible, so it can be read and improved by the people who use it.
10. **Specified, tested and documented, or it does not exist.** A feature is finished only when its behavior is written as testable scenarios, covered by tests, and described in the documentation.

---

## Roles

Roles describe responsibility, not rank. Everyone is held to the same Code of Conduct.

**Contributor.** Anyone who opens an issue, comments, reviews, or sends a pull request. No permission is needed to be one.

**Reviewer.** A contributor trusted to review pull requests and triage issues, so that maintainers can focus. Reviewers can approve a change but cannot merge it.

**Maintainer.** Can merge pull requests, decide proposals within the rules of this charter, and enforce the Code of Conduct. Maintainers are expected to review, to guide newcomers, and to protect the design principles.

**Lead maintainer.** The project founder holds final say on language design and on the direction of the project, and breaks ties when maintainers cannot agree. This is a deliberate choice for the early stage: a language needs a coherent vision, and the lead is accountable for it. The lead is expected to explain decisions in writing and to listen.

**Becoming a reviewer or maintainer.** There is no application form. People who show sustained, high-quality contributions, good judgment and respect for others are invited by a maintainer, and the lead confirms. Maintainers who step back for a long time may be moved to emeritus status, with thanks, and can return when they wish.

---

## How decisions are made

**Day to day: lazy consensus.** Most changes (fixes, tests, documentation, small improvements) need no ceremony. A pull request with an approval and passing tests can be merged unless someone raises a reasoned objection.

**Language and library design: proposals.** Anything that changes what Zila Lang programs mean, or the public shape of the standard library, goes through the proposal process in [the next section](#the-life-of-a-proposal). A maintainer decides after the discussion period; the decision is recorded with its reasons.

**What makes a proposal acceptable.** A proposal is judged on whether it:

- fits the [design principles](#design-principles);
- solves a real problem shown by a concrete example, not a hypothetical one;
- can be specified as scenarios a test can check;
- is worth its cost in complexity for the language, the compiler and the documentation;
- has a credible plan for being built and tested, and someone willing to do it.

A good idea can still be *deferred* because it is not the right time. A rejection is never a judgment on the person who proposed it.

**Disagreement among maintainers.** Maintainers seek agreement first. If they cannot agree after a fair discussion, the lead decides and explains why.

**Transparency.** Decisions happen in public (issues and pull requests), except for Code of Conduct matters, which are confidential.

---

## The life of a proposal

```text
Proposal issue ──► Discussion ──► Decision ──► Spec ──► Build ──► Archive
```

1. **Proposal.** An issue titled `Proposal: …` states the *why*, the *what* (with example code), the alternatives and the impact.
2. **Discussion.** Open for at least **7 days**, or **14 days** if it breaks existing programs. The author is expected to take part and to improve the proposal with the feedback.
3. **Decision.** A maintainer marks it *accepted*, *rejected* or *deferred* and writes the reason. Rejected and deferred proposals stay on record, so the same question does not have to be re-argued from scratch; reopening one needs new arguments.
4. **Spec.** An accepted proposal becomes an OpenSpec change in `openspec/changes/`: `proposal.md`, specs with *WHEN … THEN …* scenarios, `design.md` and `tasks.md`. A maintainer reviews the spec before building starts.
5. **Build.** The tasks are implemented in pull requests, each with tests and documentation. The change is complete only when every task is done and verified.
6. **Archive.** The change moves to `openspec/changes/archive/` and its specs join the living specification in `openspec/specs/`.

Small changes may combine steps, but never skip the principle behind them: *decide, then specify, then build*.

---

## Compatibility and versions

- **Before 1.0**, the language may change in ways that break programs. Every breaking change must be named as such in its proposal, include a migration note, and have the longer discussion period. We will keep such changes few and deliberate.
- **While the standards are still being set**, all Zila Lang code lives in this repository, so a breaking change is paid for by rewriting that code (the standard library, `os/`, the examples and the tests) in the same change. There is no automatic migration tool yet. The rewritten code and its tests are the proof that the migration note is correct.
- **Before 1.0**, a short list of foundational decisions is frozen (memory model, error model, visibility, concurrency and transfer rules). They cannot change without a new major version, because the package manager depends on them.
- **From 1.0**, projects declare an `edition` in their manifest, so the compiler can evolve without breaking older libraries. Existing programs keep working across minor versions. Breaking changes need a deprecation period of at least one minor version with a clear warning, and are bundled into a new major version.
- Versions follow `MAJOR.MINOR.PATCH`. The language specification, the compiler and the standard library are versioned together until there is a reason to split them.

---

## Quality bar

Every change to the project, from anyone, meets the standard described in [CONTRIBUTING.md](CONTRIBUTING.md). In short: it has tests, the documentation follows it, the compiler gains no external dependency, no generated files are committed, and the author can explain every line.

These rules apply to maintainers and to the lead as much as to newcomers.

---

## Conflicts

1. **Talk first.** Most disagreements are misunderstandings. Say what you mean, ask what the other person means.
2. **Bring in a reviewer or maintainer** if the two of you cannot settle it, and ask them to listen to both sides.
3. **Technical deadlock** goes to the maintainers, and then to the lead, who decides and explains.
4. **Conduct problems** are handled under the [Code of Conduct](CODE_OF_CONDUCT.md), privately and fairly.

Nobody is penalized for disagreeing in good faith.

---

## Licensing

The project is intended to be released under the **MIT** license or the **Apache License 2.0**, at the user's choice.

- Contributions are accepted under the same terms: by submitting one, you confirm you have the right to do so and that it may be distributed under them.
- Programs written *in* Zila Lang belong entirely to their authors. The compiler's license does not apply to the programs it compiles, nor to the small runtime that the compiler links into them.
- Code copied from elsewhere must have a compatible license and be credited. If in doubt, ask before including it.

---

## How governance will grow

The founder-led model suits a project this young, and it is meant to be temporary.

- **When there are at least three active maintainers besides the founder,** a **Language Design Committee** of three to five members will be formed from the most active contributors. It will decide language proposals by vote, with the lead keeping a veto only for violations of the design principles.
- **By the 1.0 release,** this charter will be revised into a fuller constitution with written election and succession rules, and the public roadmap will be set with the community.

The best way to a seat at the table is simple: sustained, careful contribution and good company.

---

## Amending this charter

This document can be improved like any other. A change to it needs:

1. a pull request that explains what changes and why;
2. at least **14 days** of open discussion;
3. approval from the lead maintainer **and** a majority of the maintainers.

Typos and clarifications that do not change meaning can be merged like any other fix.
