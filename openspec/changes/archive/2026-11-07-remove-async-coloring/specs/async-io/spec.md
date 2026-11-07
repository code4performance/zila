## ADDED Requirements

### Requirement: Awaitable operations work from any function
Every awaitable operation of the library (`sleep`, `timeout`, `all`, `race`, `fs.read`, `fs.write`, and the `net` operations) SHALL be usable with `await` from any function, method or lambda, not only from `async func` and `main`. The operation SHALL NOT block other tasks wherever it is awaited.

#### Scenario: Reading a file in a plain function
- **WHEN** a plain function `load(path: text) -> text or error` does `return await fs.read(path)` and `main` calls it
- **THEN** it yields the file contents, and a spawned ticker task keeps running during the read

#### Scenario: Sleeping inside a lambda
- **WHEN** three lambdas that each `await sleep(100)` are called one after another from tasks started with `spawn`
- **THEN** they overlap and finish after about 100 milliseconds in total
