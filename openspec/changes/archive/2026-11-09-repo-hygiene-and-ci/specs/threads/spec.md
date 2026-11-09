## MODIFIED Requirements

### Requirement: Same behavior on Windows and Linux
`spawn_thread`, `Thread`, `Mutex`, `Atomic`, `shared` and thread-safe channels SHALL behave the same on Windows and Linux, and deterministic example programs that use them SHALL print the same output on both.

> **Not yet met.** The Linux thread layer (`clone`, `futex`, a per-thread scheduler on `epoll`) is not written: `spawn_thread` works on Windows only and is rejected for `--target linux` (`tests/threads.rs`, `linux_target_says_threads_are_not_available_yet`). The scenario below describes the goal and cannot pass until that layer exists (see the roadmap in `README.md`).

#### Scenario: Examples agree
- **WHEN** the thread examples are built for both targets and run
- **THEN** their output is identical
