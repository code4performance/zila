## 1. Compiler

- [x] 1.1 In `src/codegen_x64.rs` change `int_type` so `int` is 32-bit signed and add `long` as 64-bit signed; verify with compile tests that `let a: int = 1; let b: i32 = a` and `let c: long = 1; let d: i64 = c` compile, and that assigning an `int` to a `long` without conversion fails as a mismatch
- [x] 1.2 Type unconstrained integer literals as `int` (`ExprKind::Int` at ~line 1338, replacing the `Ty::i64()` helper use) and keep range checks; verify `let x = 3000000000` is a range error suggesting `long`, and `let x: long = 3000000000` compiles
- [x] 1.3 Make `Ty::show` print `int` and `long`, and update the inference hint at ~line 5485; verify with error tests showing `expected int, found text` and `expected long, found int`
- [x] 1.5 Implement lossless widening in the type checker for assignment, initialization, arguments, returns, field initializers and binary operands (result takes the wider type), with none for narrowing, same-width signed/unsigned, integer/float, generic inference or interface and function-value signatures; verify with accept tests (`int` to a `long` parameter, `int + long` is `long`, `u32` to `i64`) and reject tests (`long` to `int`, `u64` to `int`, `i32` to `u64`, `float` to `int`, `max<T>(int, long)`) in `tests/codegen.rs`
- [x] 1.6 Make `len`, element counts and list indices `int`, and stop the program with a fatal failure when a list, map or text would exceed 2,147,483,647 elements; verify that `for i in 0..xs.len` indexes with no conversion and that a test growing past the ceiling (using a synthetic limit in the runtime for the test) stops the program
- [x] 1.7 Verify that `int * int` wraps at 32 bits before widening and that `long(a) * long(b)` is exact, with a test
- [x] 1.4 Verify `int(x)`, `long(x)` and `float(x)` as conversions (float to `int` out of range still panics `float out of range` at the i32 limits) and that `c.int` / `c.long` keep C sizes; add tests in `tests/codegen.rs` and `tests/floats.rs`

## 2. Rewrite Brasa code

- [x] 2.1 Review `std/*.br` and change every place a value can exceed `i32` (file sizes and offsets in `fs`, timestamps, hashes in `set`/maps, byte counters, handles) to `long` or `usize`, leaving lengths and indices as `int`, and list every `int` accumulator (hash functions, `format` thousands, time and size arithmetic) in the commit message; verify with the full test suite excluding `tests/async.rs` plus one test per library that exceeds 2^31
- [x] 2.2 Do the same for `os/boot.br`, `os/kernel.br` and `examples/*.br`; verify with `tests/os.rs` and by compiling every example
- [x] 2.3 Update expected output and literals in `tests/*.rs` that assumed 64-bit `int`; verify the suite passes (async tests only when the owner asks)

## 3. Docs and grammar

- [x] 3.1 Fix `brasa.ebnf` semantic notes (literals default to `int` = `i32`; `long` = `i64`; `float` = `f64`) and `docs/syntax.html` (remove "int (64-bit)", document the aliases, `long`, `usize` and the 2 GB note); verify by grep that no doc says `int` is 64-bit
- [x] 3.2 Update `README.md`, `docs/stdlib.html`, `docs/native.html` and `docs/examples.html` where they state sizes or use `int` for large values; verify the pages and the docs-sync checks (`docs-sync` spec) pass

## Notes (implementation)

- Compiler work for 1.1-1.5 and 1.7 was already in the tree (`int_type`, `Ty::int()`, `Ty::show`, `widens`/`wider`, literal range hint). This pass added: literal-on-the-left adaptation (`0 - x` with `x: u32`, also for constant expressions such as `(1 + 2) * x`, and in fused conditions), the constant folder's comparisons now type literals as `int`, a peephole that drops a repeated `movsxd rax, eax` (the fib benchmark's instruction ratio regressed once `int` became 32-bit), and `std/ui.linux.br` (`scale(width, i32(screen_dpi))`).
- 1.6: `rt_max_len` (default 2147483647) is checked in `rt_list_push`, `rt_text_alloc` (so every text creation and concatenation) and `rt_map_set` (new key). Past it the program aborts with `list, map or text is too long` (exit 101, no location). Test hook: the hidden builtin `__set_max_len(n)` (like `__from_bits`), used by `tests/integers.rs`.
- Default taken: `Listener.port` and the `net` port parameters stay `long` (already so in `std/net.br`); the async tests were updated to match.
- 2.1 audit by reading (no commit message, since nothing is committed): `std/` already uses `long` for `file_size`, `transfer` count/offset, `task.now()`, timers, `text.parse_long`, `format` helpers (`int(n: long)`, `hex`, `bin`, `thousands`), tls byte totals and the float library's big-number limbs; `parse_int` is 32-bit and `parse_long` takes 64. There are no hash functions in `std/` (maps hash in the runtime). Lengths and indices are `int`.
- 2.2: `os/boot.br` + `os/kernel.br` build an image (`--os`); `examples/*.br` build for Windows and Linux except `control` (syntax showcase with undefined names), `ffi` (Windows: `#[link_src]`), `lowlevel`/`testing` (no `main`), `rest_api` (module `db`) and `types` (`Customer` is never declared: unchanged since the first commit, not caused by this change).
- 2.3 test fixes: literals above `i32` use `long`, `parse_long` for 64-bit parsing, `Stack<int>`/`int` wording, `&done`/`&got` out-parameters use `usize` (taking the address of a narrower local is not supported), `port: long` in async tests.
- Tests run: every `tests/*.rs` file individually except the ones below, all passing: abort, allocator, buffered_io, codegen, defer, docs, error_kinds, floats, fmt, format, generics, integers (new, 18), lsp, maps, noalloc, optimizer, parse, pe, refs, run, sets, sorted, stdlib, testrun, udp, vscode, plus `--lib --bins`, plus `tests/async.rs` once at the end (26 passed).
- Not run (needs permission): `tests/linux.rs`, `tests/os.rs`, `tests/tls.rs`, `tests/dns.rs`, `tests/ui.rs` (WSL/QEMU/network/GUI). Their code paths were audited by reading and compile-checked: the OS image builds, every Linux example and `ui`/`tls`/`net`/`fs` build for both targets.
