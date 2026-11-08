## Context

Attributes are parsed on declarations (`Decl.attrs`, for `#[naked]`, `#[export]`, `#[cfg]`, ...). A function that returns `error` returns `return` for success and `return error("text")` for failure, and `try f()` propagates; `match f() ok -> ... err(e) -> ...` tests the result of an `-> error` call (the `ok` pattern binds nothing for it). `panic(msg)` is an intrinsic that prints and exits with 101. `main` may return `error`. Native builds go through `compile_to_native_with(&File, exe, &CompileOptions)`; the CLI does not give programs arguments or environment, and there is no exception catching, so a crash ends the process.

## Goals / Non-Goals

**Goals**
- Tests written in Zila Lang, run in one process per file, with a clear pass/fail output and exit status.
- Assertions that fit the language's error model rather than a new control-flow mechanism.

**Non-Goals**
- Isolating or catching crashes (one crashing test stops the file's run), test filtering, parallel runs, fixtures, benchmarks, running tests in the interpreter, test discovery in directories, coverage.

## Decisions

1. **Assertions are errors, not panics.** A failed `assert` returns an error from the enclosing function, exactly like `return error(...)`, so the runner can continue with the next test and helpers compose with `try`. This is why they are only legal in functions returning `error` or `T or error`; the alternative (panicking in functions without a result) would stop the whole run and make the same call mean two things.
2. **A rewrite in the code generator.** In the statement compiler, a call statement `assert(...)` / `assert_eq(...)` (when the program defines no function of that name) is replaced by AST built on the spot and compiled as ordinary statements inside a new scope:
   - `assert(c)` and `assert(c, m)`: `if not (c) return error("line N: assertion failed") end`, or with the message `"line N: m"` when `m` is a text literal (an interpolated message is allowed and is evaluated only on failure);
   - `assert_eq(a, b)`: `let __l = a`, `let __r = b`, `if __l != __r return error("line N: assertion failed: left {__l}, right {__r}") end` (the arguments are evaluated once; text interpolation prints integers, floats, booleans and text).
   The enclosing function must have a result of `error` or `T or error` (checked from the function state; the message tells the user to write `-> error`). `N` is the line of the call.
3. **Finding tests.** `src/testrun.rs` parses the file with the normal parser and collects `Decl`s that are functions with a `test` attribute, in order. It rejects (naming the function) parameters, `async`, generics, receivers and any return type other than none or `error`, and files whose package is not `main`.
4. **The generated entry point.** The runner removes the file's `main` and appends a generated `main() -> error` (built by parsing a text template, so it needs no hand-made AST):
   ```
   func main() -> error
       var passed = 0
       var failed = 0
       match NAME()            // for a test that returns error
           ok -> ...print("ok NAME") ... passed += 1
           err(e) -> print("FAIL NAME: {e.message}"), failed += 1
       end
       NAME2()                 // for a test that returns nothing
       print("ok NAME2") ...
       print("test result: {passed} passed, {failed} failed")
       if failed > 0
           return error("{failed} test(s) failed")
       end
       return
   end
   ```
   so one process runs every test of the file. A test that returns nothing cannot fail by assertion (they are illegal there) and is reported `ok` if it returns.
5. **Building and running.** For each file the runner builds the modified AST to an executable in the temp directory (`compile_to_native_with`, honouring `--no-opt`), runs it, forwards its output and removes it. The run is reported as aborted when the process exits with a code other than 0 or 1 or when it never printed the `test result:` line (a crash or a panic): the CLI then prints `run stopped early (exit code N)` after the output.
6. **Exit status and output.** The CLI prints `running N test(s) in <file>` before each file's output (so several files are readable), then the program's lines. The exit status is 1 if any file had a failure, did not compile, or stopped early.
7. **Native only.** If there is no Visual Studio toolchain, the runner reports the same message as `--build` and exits 2.

## Risks / Trade-offs

- A crash in one test hides the tests after it; the report says where the output stopped.
- `assert` only in error-returning functions is a restriction that may surprise people coming from other languages; the compile error explains it, and the documentation shows the shape of a test.
- The generated `main` uses the names of the tests directly, so two tests with the same name in a file are a compile error from the language itself (already the case for functions).
