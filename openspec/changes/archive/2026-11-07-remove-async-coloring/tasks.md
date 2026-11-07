## 1. Compiler

- [x] 1.1 `src/codegen_x64.rs`: remove `FnState::allow_await`, its assignment in the function prologue (`self.fs.allow_await = fd.is_async || ...`), the `allow_await` handling in `lambda()`, the `next_lambda_await` field and its set in `emit_async_launch`, and the check in the `ExprKind::Await` arm. Verify: `cargo build` has no warnings; `cargo test --test async` (run explicitly, it is left out of routine full runs).
- [x] 1.2 Reject a direct `await` in a method named `drop` ("`await` cannot be used in a `drop` method") and in a function that is `#[export]`ed or converted to an `extern` callback type ("... called from C"). Verify: two negative tests in `tests/async.rs` compare the error text.
- [x] 1.3 Confirm the library registration pulls `task`/`future` in when `await` appears only in a plain function (`file_uses_concurrency` collects `$await` through `collect_expr`). Verify: a program with `await` only in a non-async, non-main function builds and runs.
- [x] 1.4 `src/regalloc.rs` and `src/testrun.rs`: no behavior change expected; check that a `test_` function containing `await` runs under `brasa test`. Verify: a test in `tests/testrun.rs` style with `await sleep(1)` passes.

## 2. Grammar and standard library

- [x] 2.1 `brasa.ebnf`: replace the semantic rule "`await` is allowed only in an `async` function and in `main`" with the new one (any function; not in `drop` or C callbacks; `async` functions take no `ref` parameters). Verify: `cargo test --test docs --test parse` pass and the text matches the spec.
- [x] 2.2 `std/task.br`, `std/task.linux.br`, `std/future.br`: comments only (await may run in any function; task 0 is the main program). Verify: the diff has comments only; `cargo test --test stdlib` unchanged.
- [x] 2.3 Review `std/fs*.br`, `std/net*.br`, `std/tls*.br`, `std/ws.br`, `std/bufio.br` and `os/`: turn any helper made `async` only to be allowed to await into a plain `func` (keep `async` where a Future is wanted). Verify: `cargo test --test stdlib --test tls --test dns --test udp --test buffered_io` pass (network tests may skip offline).

## 3. Examples, tests, docs

- [x] 3.1 `examples/async.br`: make `fetch_title` a plain `func` that awaits, and add a `map` with an awaiting lambda next to the existing `all([...])` line to show both forms. Verify: `brasa-parse examples/async.br -o async.exe` builds and the old output lines are unchanged.
- [x] 3.2 `tests/async.rs`: in `errors_flow_through_await` replace the assertion that `twice` is rejected with the fixed program (it now compiles and prints the same output). Add `await_works_in_plain_functions_and_map`: a plain `func load(n: int) -> int` with `await sleep(10)`, `main` calling it, `[1, 2, 3].map(func(i) await work(i) end)` through `list.map`, a defer that awaits, and `main` awaiting from a nested call while a spawned task prints. Add the `drop` and `#[export]` negative tests from 1.2 and a deadlock test where a plain function awaits a never-completed future. Verify: `cargo test --test async`.
- [x] 3.3 `docs/async.html`: rewrite the rules list (await in any function and what it suspends, where it is forbidden, `async func` as the short form that starts a task), show the two forms for fetching several things (`map` with `await` vs `await all(map(async func))`), and the caveats (globals shared across an await, callbacks and `drop` cannot await). Same wording in `docs/syntax.html` (Async section), `docs/concurrency.html` and `README.md`. Verify: `cargo test --test docs`; read each page against the spec.
- [x] 3.4 Run the routine suite without async (`cargo test` skipping `async`) and `openspec validate remove-async-coloring --strict`. Verify: both green.
