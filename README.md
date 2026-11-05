# Brasa front end (Rust)

Lexer, AST and recursive-descent parser for Brasa, following `brasa.ebnf`.
No dependencies (std only).

```
cargo build --release
cargo test

./target/release/brasa-parse examples/rest_api.br            # OK + summary
./target/release/brasa-parse examples/hello.br --ast          # dump the AST
./target/release/brasa-parse examples/hello.br --tokens       # dump tokens
./target/release/brasa-parse examples/hello.br --run          # execute with AST interpreter
```

Errors show file, line, column and the offending line:

```
error: expected 'end', found end of file
  --> prog.br:4:1
```

## Layout

| File | What it does |
|---|---|
| `src/lexer.rs` | Tokens, newline collapsing, text literals with `{expr}` interpolation |
| `src/ast.rs` | AST types (every statement and expression carries its line) |
| `src/parser.rs` | Declarations, types, statements, expressions with the grammar's precedence |
| `src/interp.rs` | AST tree-walk interpreter for executing Brasa programs |
| `src/codegen_x64.rs` | Native compiler: type checking, x86-64 code generation, runtime, `ml64`/`link` driver |
| `std/*.br` | Standard library written in Brasa: `ui` (widgets), `task` (scheduler, channels), `future` (async/await), `fs` (files), `net` (TCP), `sys`/`ws` (shared helpers, Winsock) |
| `src/main.rs` | `brasa-parse` command |
| `tests/parse.rs` | Precedence, newline rules, match arms, generics, error positions |
| `examples/*.br` | Programs from the design discussion (REST API, UI, low level, types) |

## Design notes

- `if` and `match` are expressions, so they appear as `ExprKind::If/Match` (as statements they are `StmtKind::Expr`).
- Newlines end statements. They are skipped inside bracketed lists, after binary operators and after `=`. A next line starting with `.` or `?.` continues a method chain.
- A multi-line `match` arm body ends at the next line that looks like `pattern ->` (speculative lookahead) or at `end`.
- `>>` and `>=` are split when closing generics (`Map<text, List<i32>>`).
- Record literals are recognised as `Path {` after a name or dotted name.
- Comparisons do not chain (`a < b < c` is an error).

## Not implemented yet

- `html ... end` templates (needs a markup-aware lexer mode)
- Explicit generic arguments in calls (`first<i32>(x)`); types are passed as values (`conn.query(Customer, ...)`)
- Semantic analysis: name resolution, type checking, exhaustive `match`, `?` only in `!` functions
- Error recovery: the parser stops at the first error

## Native compiler (`--build`)

```sh
./target/release/brasa-parse examples/ui.br -o ui.exe     # native Windows x86-64 executable
./target/release/brasa-parse examples/hello.br --asm      # show the generated assembly
```

Brasa compiles straight to x86-64 machine code. `src/codegen_x64.rs` emits
assembly, and `ml64` + `link` from Visual Studio (found automatically with
`vswhere`) assemble it into a PE executable. There is no C or Rust in between
and no C runtime: the program imports only the Windows DLLs it uses. A small
runtime written in assembly is linked into every program (allocation, text
and lists).

The standard library is written in Brasa. `use ui` compiles [`std/ui.br`](std/ui.br)
together with the program. That file talks to Win32 directly through
`extern "system"`, `#[c_layout]` records and a Brasa `wndproc` callback.

Supported in the native backend:

- Integers (`int`, `byte`, `i8`–`u64`, `usize`), `bool`, `text` with interpolation,
  records (copied on assignment, shared through `ref`), `[]T` lists, raw pointers
- `enum` with or without data, and `match`: exhaustive for enums and results,
  with `|` alternatives, number/text patterns, `_` and catch-all names
- Errors: `-> T or error`, `try call()`, `error("message")`, `e.message`,
  `match r ok(v) -> ... err(e) -> ...` and `call() ?? default`
- Functions, methods, default and named arguments, variadic `...T`
- Closures (they capture by value; the return type is inferred) and top-level
  functions passed as `extern` callbacks
- `spawn f(x)`, `channel(T)`, `ch.send(v)`, `ch.receive()`, `ch.close()`: tasks
  are Windows fibers scheduled by [`std/task.br`](std/task.br) (written in Brasa)
- `async func` / `await` with `Future<T>`, `sleep`, `timeout`, `await all(xs)`, `await race(xs)`,
  and the `fs` and `net` libraries (see below and [`examples/async.br`](examples/async.br))
- A record may define `func (ref x: T) drop()`, which runs when its last reference goes away
- `if` (also as an expression), `while`, `for` over ranges and lists,
  `break`/`continue`, `unsafe`, `asm`, and `extern` calls (these require `unsafe`)
- Properties: when a record has no field `x` but has a method `x()`,
  `r.x` calls it, and `r.x = v` calls `set_x(v)`

Memory is managed by **reference counting**: heap values (text, records, lists,
closures, enums with data, results, channels) are freed as soon as the last
reference goes away, including on early `return`, `break` and `try`. There is no
garbage collector and no pause. Reference cycles leak. `live_objects()` returns
the number of live heap objects, which makes leaks easy to test.

Tasks are cooperative: a task runs until it waits (`receive`, `await`), calls
`task.yield()` or finishes. `send` never blocks. When every task waits, the
program stops with a deadlock error. Each finished task leaves one small record
behind. The `ui` message loop does not yet run other tasks.

Not supported yet: floats, generics, maps, `defer`, fixed arrays.

## Async

```
use fs
use net

async func double(n: int) -> int
    await sleep(100)
    return n * 2
end

func main() -> error
    let a = double(1)                       // starts now
    let b = double(2)
    print(await a + await b)                // about 100 ms, not 200
    try await fs.write("notes.txt", "hi")   // files and TCP never block other tasks
    print(try await fs.read("notes.txt"))
    return
end
```

An `async func` runs as its own task and returns a `Future<T>`; `await` waits for it without
blocking the others and works with `try` (`try await f()` for `-> T or error`). `await` is only
allowed in an `async func` and in `main`. `sleep(ms)`, `timeout(ms, future)`, `await all(xs)` and
`await race(xs)` handle time and groups of futures. `fs.read`/`fs.write` use overlapped Windows file
I/O, and `net.listen`/`accept`/`connect`/`read`/`write`/`close` use overlapped Winsock sockets, all
on one completion port polled by the scheduler.

Limits: one thread (no parallelism), no cancellation (a future that timed out keeps running), a
future can be awaited once, looking up a host name blocks every task, TCP over IPv4 only, Windows
only. When every task waits and no timer or I/O is pending, the program stops with a deadlock error.

## Simplified syntax

Brasa keeps one way to write each thing, so a beginner has less to memorize:

| Instead of | Brasa uses |
|---|---|
| `union` and `enum` | one `enum` (`Red, Green` or `Circle(int)`) |
| `T!` and `expr?` | `T or error` and `try expr` |
| `func(x) -> x * 2 end` | `func(x) x * 2 end` |
| `i32`, `f64`, `u8` for everyday code | `int`, `float`, `byte` (sized types stay for FFI) |

## Brasa OS (bootloader + kernel in Brasa)

`os/boot.br` is a 512-byte boot sector and `os/kernel.br` a 64-bit kernel with a
text console and a shell (`help`, `echo`, `calc`, `mem`, `clear`, `reboot`, `halt`).
The compiler assembles everything itself ([`src/asm_x86.rs`](src/asm_x86.rs)), so no NASM
or linker is involved.

```sh
brasa-parse --os os/boot.br os/kernel.br -o brasa-os.img
.\osun.ps1            # boots it in QEMU (VGA window)
.\osun.ps1 -Serial    # headless: the shell runs on your terminal
```

How it boots: the BIOS loads the sector at 0x7C00 in 16-bit real mode; it reads the
kernel from disk to 0x10000, enters protected mode, builds identity-mapped page tables,
enables long mode and jumps to the kernel. The boot code is Brasa source whose
`#[naked]` function holds `asm` blocks, because the real-mode and mode-switching
instructions cannot be produced by the 64-bit code generator. The kernel is ordinary
Brasa: text, lists, enums, `match`, `try`, closures and reference counting all work
(the compiler links a bare-metal runtime with its own heap). `print` goes to a
`console_write(s: text)` function the kernel defines; here it writes to the VGA screen and COM1.

The kernel needs no operating system, but it needs a machine: use QEMU
(`winget install SoftwareFreedomConservancy.QEMU`, or unpack the installer into
`%USERPROFILE%\qemu`). `tests/os.rs` boots the image and checks the shell's output.

## FFI (C and assembly)

The parser accepts `extern "C" func ...` (with `...` varargs and ABI strings),
`extern "C" func(...)` callback types, stacked attributes (`#[link]`, `#[export]`,
`#[naked]`, `#[cfg]`) and `asm ... end` blocks with `in/out/inout/clobber`.
See `examples/ffi.br` and `brasa-ffi.md`.

## Documentation site

`docs/` is a static site (open `docs/index.html`; no server needed). It stays in step with the code:

- The example programs on the site come from `examples/`. After changing one, run
  `brasa-parse --docs` to refresh the blocks; `cargo test` (`tests/docs.rs`) fails if a block, a
  command-line option or a keyword differs from the code, or if a page or link is inconsistent.
- Every OpenSpec change must plan the docs it affects (`openspec/config.yaml` has the rule).

## Editor support (VS Code / Antigravity)

Syntax highlighting and language configuration for `.br` files are provided in `editors/vscode`:
- TextMate grammar (`syntaxes/brasa.tmLanguage.json`) covering keywords, types, string interpolation `{expr}`, attributes `#[...]`, inline `asm` blocks and operators.
- Language configuration (`language-configuration.json`) for comments, bracket matching, indentation and folding.
- Installed directly into VS Code and Antigravity extension directories.
