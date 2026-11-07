## Context

Today `panic` is a built-in handled in `src/codegen_x64.rs` (`name == "panic"`, calling `rt_panic`, which writes `panic: <text>` with `rt_write_out` and exits with 101 through `rt_exit`). The run-time checks that behave the same way all jump to it: `rt_index_oob`, `rt_take_empty`, the zero-divisor stubs in the Windows and Linux runtimes (`rt_msg_divzero`), `rt_msg_oom` in the allocators, `rt_msg_nokey` in `src/runtime_map.asm`, and `rt_msg_floatrange` for `int(x)`. None of them knows a file or line. The only `panic(` in Brasa code is `std/text.br` (`split`); `os/`, `examples/` and the other `std/` files have none. The `panic!` calls in `tests/*.rs` are Rust and unrelated. Integer arithmetic already wraps in generated code and in constant folding (`wrapping_add` and friends); only `brasa.ebnf` still says "panic in debug". `assert`/`assert_eq` are not panics: `assert_stmt` rewrites them into `return error("line N: ...")` and rejects them outside functions returning `error`.

## Goals / Non-Goals

**Goals:**
- One word for fatal stops (`abort`), one behavior (message, exit 101, nothing runs), one place in the specs.
- Overflow never fails; a checked alternative exists in the library.
- A non-fatal way to read a list element.
- Test runner behavior unchanged.

**Non-Goals:**
- Catching or recovering from an abort, unwinding, running `defer` or `drop` (explicitly the opposite).
- Overflow traps, or debug/release differences (there is no `--fast` flag today; the rule is deleted from the grammar).
- Wider integer types and their `checked_*` variants (`add-int-long-aliases`), the error kinds themselves (`add-error-kind`).
- Replacing `m[key]` with a failing form: it stays, as an abort, and `has`/`get(key, default)` remain the handleable forms.

**Depends on:**
- `add-error-kind`: `checked_*` return errors with kind `Overflow` and `InvalidInput`. If it has not landed, the tasks use plain `error("...")` messages and switch to kinds when it does.
- `remove-interpreter`: the `panic` case in `src/interp.rs` is renamed to `abort` only if the interpreter still exists when this lands; otherwise that task is dropped.
- `add-direct-pe-output`: the Windows runtime text changes (`rt_abort`, stderr write) are made in the assembly that change keeps assembling, so they are independent; only the verification commands differ.
- `remove-async-coloring`, `add-threads-and-shared`: no code overlap; both inherit that an abort in any task or thread ends the whole process.
- `add-no-alloc-and-allocator`: an allocator that can fail returns errors for allocation; the built-in heap still aborts when exhausted.
- `add-text-bytes-and-char`: decides whether text gets `get`; not covered here.

## Decisions

**1. `abort` is a built-in function, kept user-callable.** `abort("message")` has type `nothing` and never returns. It replaces `panic` one for one. Having it callable means a program that finds a broken invariant has an honest way to stop, and it spares the library from inventing tricks such as a deliberate division by zero. Alternatives: a statement keyword (needs grammar for no benefit), or no user-callable form (pushes people to write `1 / 0`). A user function named `abort` takes precedence, as `panic` did.

**2. Message: `abort: <file>.br:<line>: <message>` on standard error, exit 101.** Standard error, not standard output: today `panic: ...` goes through `rt_write_out`, which mixes with program output. The language has no stderr writer, and this change does not add one: the *runtime* writes the abort message (a new `rt_write_err`: `GetStdHandle(-12)` on Windows, file descriptor 2 on Linux, the kernel console on bare metal) and Brasa code cannot. The `error: <message>` line of a failing `main` is not an abort and stays on standard output with exit status 1. Tests that capture both streams keep working; pipelines get clean output. `<file>` is the module name the compiler knows (`main.br` for the program's own file, whatever the file is called; `text.br` for `std/text.br`). The file is the module file as the compiler knows it (`std/text.br` sites print `text.br`). Exit 101 is kept so `linux-target`, `linux-parity` and existing tests keep their number.

**3. Locations are compiled in, per site.** Each checking site (index, division, remainder, `take`, map `m[key]`, `int(float)`, `assert` outside tests, and the `abort(...)` call) gets a constant `file:line` text in the data section, and the failing path loads it into a register before jumping to `rt_abort`. Shared runtime routines that fail are given the location by their caller, which already knows the line (`self.line`), in a register that is free at the call: `rdx` for `rt_list_take` and for `rt_abort` itself (with the message in `rcx`), `r8` for `rt_map_get`, `r9` for `rt_list_get`, `rt_list_set` and `rt_text_byte`; the division check loads `rdx` before the zero test. The internal callers that cannot fail (defer lists, loops over a list) pass nothing. Cost is zero on the success path and a few bytes of data per site. Alternatives: a global "current line" store before every statement (a store on the hot path, rejected), or a return-address to line table searched at abort time (precise but breaks across inlining and tail jumps; revisit with debug info). Out of memory is raised inside the allocator, which has no source line, so it prints `abort: out of memory` with no location. This is the one exception to "file and line" and the spec says so.

**4. No cleanup runs.** `rt_abort` writes the message and calls `rt_exit(101)` directly. It does not touch the deferred-closure lists, does not release anything, and does not flush `bufio` buffers or C's `stdout` (an unflushed Brasa `Writer` is lost, as in any crash). This is simplest and cannot itself fail, which matters when the heap is what ran out.

**5. Overflow wraps, always.** No code change for `+ - *` (already wrapping, in folding and at run time). The one trap left is hardware: `MIN / -1` and `MIN % -1` raise a divide fault on x86-64. The check emitted before `idiv` gets a second case that yields `MIN` (and 0 for `%`) when the divisor is -1. Narrow integer types wrap at their width, as `norm` already does.

**6. `checked_*` in `std/math.br`, written in Brasa.** `checked_add`, `checked_sub`, `checked_mul`, `checked_div` return `int or error`. Overflow is detected without compiler support: add and sub by the sign test on the wrapped result, mul by dividing back (with zero and `MIN * -1` handled), div by testing `b == 0` and `a == MIN and b == -1`. Kinds: `Overflow` for overflow, `InvalidInput` for a zero divisor (after `add-error-kind`). Only `int` for now; width variants follow `add-int-long-aliases`. A pure-library implementation keeps the compiler small and the functions inlinable by the existing inliner.

**7. `xs.get(i)` is a built-in list method.** Next to `take` and `clear` in the list method code of `codegen_x64.rs`: compare the index unsigned against the length (negative indexes fail the same test), return nothing or the element with the same ownership rules as `xs[i]`. It works on lists and slices (a slice is a list in the current library). *Reality check (found while implementing):* optionals in the compiler are only supported for reference types (`T?` is a nullable pointer; `int?` is a compile error), so "returns `T?`" cannot hold for a list of integers. The method therefore returns `T?` when the element type can be `nil`, and `T or error` (kind `InvalidInput`, `index out of bounds`) when it cannot, so `xs.get(i) ?? 0` reads the same for both. Real optional scalars are a language change of their own and are not part of this change; the spec says so.

**8. `assert` depends on test mode.** `assert_stmt` currently rewrites to `return error(...)` and requires an error-returning function. Under `--test` that stays exactly as is, so a failing assertion fails one test and the run continues. In a normal build `assert` and `assert_eq` compile to an abort carrying `assertion failed`, the message, or the two values, at the assertion's location, and are allowed in any function. The compiler needs one internal option, `test_mode`, set by `src/testrun.rs` when it builds the test entry point; it is not a user flag. Under `--test`, in a function that does not return `error`, an assertion aborts (the harness reports the crash) instead of being a compile error; this lifts the old restriction without losing information. Alternative: abort always, even under `--test`; rejected because one failing assertion would end the file's run and lose `FAIL name: message` reporting.

**9. `int(x)` out of range aborts.** It is a bug in the program's arithmetic (the caller can test the float first), so it follows rule 3.

**10. Interpreter.** `remove-interpreter` landed first: `src/interp.rs` is gone, `--run` compiles and runs, and nothing is left to rename.

**11. `rt_abort` aligns its own stack** (`and rsp, -16`), because the failing routines jump to it from different depths. The `MIN / -1` check is emitted for signed division only when the divisor is not a literal (a literal is never `-1`); a literal nonzero divisor still skips both checks under the optimizer.

## Risks / Trade-offs

- [Existing programs and tests printing `panic: ...` break] -> Pre-1.0; this change rewrites every in-repo use and expectation (listed in tasks).
- [Message moves from stdout to stderr] -> The test helpers capture combined output; verified per task.
- [No cleanup on abort loses unflushed buffers] -> Documented; same as a crash, and the price of an abort that cannot fail.
- [Per-site location text grows the data section] -> A short string per checked site; inlining does not duplicate it because the line is that of the original source site.
- [`MIN / -1` extra check on every division] -> One compare against the divisor already loaded for the zero check; never taken in practice.
- [`get` on a list of records: copy or share?] -> Same as `xs[i]`; the task tests it rather than assuming.
