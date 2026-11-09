# continuous-integration Specification

## Purpose
Say what is checked automatically on every push and pull request, so a contribution that breaks the build, adds a warning or fails a test is found before review.

## Requirements

### Requirement: Every push and pull request is checked on Windows and Linux
The repository SHALL have a workflow that, on every push to `main` and every pull request, builds all targets, runs `cargo clippy --all-targets -- -D warnings` and runs every test target except `tests/async.rs`, on Windows and on Linux. The `async` tests SHALL run in their own job that starts after the first one succeeds. A test target added to `tests/` SHALL be picked up without editing the workflow.

#### Scenario: A clippy warning
- **WHEN** a pull request adds code that makes clippy warn
- **THEN** the check fails on both systems

#### Scenario: A new test file
- **WHEN** `tests/new_feature.rs` is added
- **THEN** the next run executes it with no change to the workflow

### Requirement: Tests needing outside services skip themselves
A test that needs a database server, QEMU, WSL or another tool the runner may lack SHALL print a skip line and pass when the tool or its environment variables are absent, so a plain `cargo test` works on any machine with Rust.

#### Scenario: No database
- **WHEN** the database variables of `tests/docker/env.sh` are not set
- **THEN** the database tests print that they are skipped and the run passes
