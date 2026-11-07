# 🔥 Brasa

> **A compiled, statically-typed systems programming language that feels like Go, thinks like Rust, and talks to C without ceremony.**

Brasa compiles straight to x86-64 machine code. No LLVM, no virtual machine, no garbage collector, and no runtime to install. The compiler is written from scratch in Rust with **zero dependencies**, it ships with its own x86 assembler, and the same toolchain that builds a 6 KB Windows program also builds a bootable 64-bit operating system.

It is young, it is honest about what it can't do yet, and it is looking for people to help it grow. **If you like compilers, language design, or just want to see what a clean-slate language looks like, come build it with us.**

### The name

*Brasa* is the Portuguese word for **ember** — a glowing coal that burns hot, long, and clean, without the noise of an open flame. That is exactly what this language aims to be: intense power, controlled heat, no waste.

The name hides two deliberate puns, both rooted in its Brazilian origin:

- **Brasa → Brasil.** The first five letters of *Brasil* are *Brasa*. This is a systems language born in Brazil 🇧🇷, and the name wears that proudly.
- **"Vamos mandar brasa!"** In Brazilian Portuguese, *mandar brasa* literally means *to send embers* — but as slang it means ***"Let's do it! Let's go all out!"*** That is the spirit behind the language: get things done, full power, no excuses.

> **Brasa** = *ember* (PT) · *Bras**a** → Bras**il*** · *"Vamos mandar brasa!"* = Let's do it! 🚀

---

## Why Brasa?

Most languages ask you to choose: simple *or* fast, safe *or* low-level, friendly *or* close to the machine. Brasa's goal is to make that choice smaller.

- **Readable first.** Blocks end with `end`. No braces, no semicolons. A beginner can read a program top to bottom.
- **Honest about failure.** There are no exceptions. A function that can fail says so in its type (`T or error`), and `try` is the one visible place where a failure travels up.
- **Honest about cost.** Memory is freed by reference counting, the moment the last reference goes away. Unsafe code must say `unsafe`. Nothing allocates behind your back.
- **One language from app to metal.** The same syntax writes an everyday program, a native window, a C library binding, and a kernel.
- **A toolchain you can read.** Lexer, parser, code generator, assembler, standard library: all in this repository, all small enough to understand.
- **Built in the open.** Every language change goes through a written proposal, a spec with testable scenarios, and tests. See [GOVERNANCE.md](GOVERNANCE.md).

---

## A taste of Brasa

```brasa
package main

type Shape = enum
    Circle(int)
    Rect(int, int)
end

func area(s: Shape) -> int
    match s
        Circle(r)  -> return 3 * r * r
        Rect(w, h) -> return w * h
    end
    return 0
end

func parse(s: text) -> int or error
    if s == "bad"
        return error("cannot parse '{s}'")
    end
    return s.len
end

async func double(n: int) -> int
    await sleep(100)
    return n * 2
end

func main() -> error
    print("areas: {area(Circle(2))} and {area(Rect(3, 4))}")
    let n = try parse("brasa")
    let a = double(n)
    let b = double(n + 1)
    print("doubled in parallel: {await a} and {await b}")
    match parse("bad")
        ok(v)  -> print("parsed {v}")
        err(e) -> print("failed: {e.message}")
    end
    return
end
```

```text
$ brasa-parse demo.br -o demo.exe
$ ./demo.exe
areas: 12 and 12
doubled in parallel: 10 and 12
failed: cannot parse 'bad'
```

A few things to notice: `match` must cover every case of an `enum`; `try` hands a failure to the caller; the two `double` calls run at the same time, so the program waits about 100 ms, not 200; and `await` never blocks the other tasks.

---

## What Brasa can do today

Everything in this list is implemented and covered by tests.

**A real native compiler**

- Compiles to x86-64 machine code for Windows. A hello-world is about **6 KB**; a native window with a text box, a button and a list is about **23 KB**.
- Needs no runtime on the target machine. The programs import only the Windows libraries they use.

**A predictable memory model**

- Reference counting frees every heap value as soon as it is unreachable, including on early `return`, `break` and `try`. There are no GC pauses.
- Records are copied when assigned (value semantics); `ref` shares. Records can define `drop` to close files and sockets automatically.
- `live_objects()` reports how many heap objects are alive, so leaks are easy to test for. The test suite uses it to check that thousands of tasks and connections leave nothing behind.
- Reference cycles are not collected: avoid building them.
- `use text`, `use list`, `use format`, `use set` and `use sorted`: `split`, `join`, `trim`, `replace`, `parse_int`, `parse_float`, `map`, `filter`, `reduce`, `sort_by` and more, written in Brasa on top of generics.
- Generics and interfaces: `func first<T>(items: []T) -> T`, `type Stack<T>`, and `T: Printable` bounds with structural interfaces. The compiler infers the type arguments and makes one copy per type used.
- `float` (`f64`) and `f32`: arithmetic, comparisons that follow IEEE 754, explicit `float(n)` / `int(x)` conversions, shortest-text printing, `use math`, and C calls through the XMM registers.
- `Map<K, V>` stores values under text or integer keys: `{ "a": 1 }` literals, `m[key]`, `has`, `get(key, default)`, `remove`, and `for key, value in m` in insertion order.
- `defer call(...)` runs a call when the function ends, however it ends (`return`, the end of the body, or a failed `try`), last in first out.

**Types that help**

- Records, enums that carry data, exhaustive `match`, optionals (`T?`, `??`, `?.`), closures, methods, default and named arguments.
- Errors as values with `T or error`, `try`, and `match` on `ok(...)` / `err(...)`.

**Concurrency without ceremony**

- Lightweight tasks (`spawn`) and channels, plus `async` / `await` with futures.
- Timers (`sleep`, `timeout`), `await all(...)`, `await race(...)`, file I/O and TCP networking that never block the other tasks, all on one scheduler written in Brasa.
- Measured: 2000 tasks at once, and 100 simultaneous TCP clients, each answered correctly.
- Tasks are cooperative and run on one thread; there is no parallelism yet.

**A standard library written in Brasa**

- `ui` (native Windows controls), `task`, `future`, `fs`, `net`. They talk to the operating system through Brasa's own `extern` declarations. No Rust or C glue.

**Systems programming**

- C interop: `extern` functions, callbacks, `#[link("lib")]`, `#[c_layout]`, raw pointers, `unsafe` blocks.
- Inline `asm` with named registers, and `#[naked]` functions.

**Brasa OS**

- A **bootable 64-bit operating system** with its boot sector and kernel written in Brasa. The whole disk image is **18 KB**. It boots in QEMU, switches the CPU to long mode, and runs a small shell on the keyboard, the screen and the serial port. The compiler assembles the machine code itself; no NASM, no linker. See [docs/os.html](docs/os.html).

**Engineering you can trust**

- 107 automated tests. The documentation site is checked against the code by tests, so it cannot quietly go stale. Every design decision lives in [`openspec/`](openspec).

---

## Ideas Brasa borrowed

Brasa does not pretend to be new in every way. It takes ideas that proved themselves and tries to combine them with less ceremony.

| From | What Brasa took | How it looks in Brasa |
|---|---|---|
| **Go** | Small language, `package` / `use`, goroutine-style tasks and channels, structural interfaces, one self-contained executable | `spawn worker(ch)`, `channel(int)`, no `implements` keyword |
| **Rust** | Enums with data and exhaustive matching, errors as values, explicit `unsafe`, no tracing garbage collector, a `drop` hook | `match`, `T or error` with `try`, `unsafe ... end`, `func (ref x: T) drop()` |
| **Elixir** | Pattern matching as the main way to take data apart, lightweight tasks, `ok` / `err` results | `ok(v)` / `err(e)` arms, thousands of tasks |
| **Swift** | Reference counting for memory, optionals, string interpolation | `T?`, `??`, `?.`, `"hello {name}"` |
| **C#, JavaScript, Kotlin** | `async` / `await` with futures | `async func`, `await`, `Future<T>` |
| **Ruby, Lua, Pascal** | Blocks closed by `end` | no braces, no semicolons |
| **C and Zig** | A first-class neighbour: C interop, freestanding programs, no hidden runtime | `extern`, `#[c_layout]`, kernels with no operating system |
| **C++** | Operators spelled as words | `bitand`, `bitor`, `bitxor`, `bitnot` |

And a few things that are Brasa's own:
- `or error` / `try`: the shape of Rust's `Result` and `?`, in words a beginner can read.
- The whole async scheduler, the networking, the file I/O and the windowing library are **ordinary Brasa code**, so you can read and change them.
- One compiler that targets both a Windows program and a bare-metal kernel.

---

## Where Brasa stands

Brasa is **pre-alpha**. The design is settling, but breaking changes are still possible before 1.0.

| Area | State |
|---|---|
| Lexer and parser | ✅ Complete for the grammar in [`brasa.ebnf`](brasa.ebnf) |
| Native compiler (Windows x64) | ✅ Works for a large, useful subset |
| Bare-metal target and Brasa OS | ✅ Boots and runs |
| `async` / `await`, timers, files, TCP | ✅ Works |
| Interpreter (`--run`) | 🔧 Runs plain programs; does not know the native-only features |
| Floating point | ✅ Works: `float`/`f64`/`f32`, `use math`, C calls; Windows x64 only |
| Generics and interfaces | ✅ Works: generic functions and records, inferred type arguments, structural interfaces as bounds |
| Maps (`Map<K, V>`) | ✅ Works: text and integer keys, insertion order |
| Text, list, format, set and sorted libraries | ✅ `use text`, `use list`, `use format` (`%` templates, padding, hex, thousands), `use set`, `use sorted` (sorted maps and sets, ordered by a function) and `use bufio` (the buffers behind line reading and buffered writing in `fs` and `net`). More to come |
| Optimizer | 🔧 Constant folding, direct operands, compare-and-branch, peephole cleanups, integer variables in registers and inlining of small functions (`--no-opt` turns them off) |
| Formatter (`--fmt`, `--fmt-check`) | ✅ Standard style for `.br` files; only whitespace changes, comments kept |
| Test runner, package manager, language server | ⏳ Not started |
| Linux, macOS, ARM64, Android, iOS | ⏳ Windows x64 and bare-metal x86-64 only |

---

## How Brasa will evolve

This is the plan, in roughly the order we intend to do it. **Every item is a place where you can help.** Difficulty is a rough guide: ⭐ is approachable, ⭐⭐⭐ is deep compiler work.

| # | Work | What it involves | Difficulty |
|---|---|---|---|
| 1 | Standard library, next steps | Formatting values of any type, UDP, TLS, async DNS. Mostly written in Brasa. | ⭐ – ⭐⭐ |
| 2 | Tooling | `brasa test`, a package manager, a language server, a richer VS Code extension. | ⭐⭐ – ⭐⭐⭐ |
| 3 | More platforms | **Linux x86-64 first** (ELF and the System V ABI), then macOS. Apple Silicon, Android and iOS need an ARM64 backend. WebAssembly later. | ⭐⭐⭐ – ⭐⭐⭐⭐ |
| 4 | Self-hosting | A Brasa compiler written in Brasa. | long term |

Beyond the compiler, contributions that make a big difference right now:
**more example programs**, **bug reports with a tiny failing program**, **documentation and tutorials**, and **tests**.

### How a change happens

1. **Propose it.** Open an issue describing *what*, *why* and the alternatives. Small fixes can skip straight to a pull request.
2. **Discuss and decide.** The community debates it; a maintainer records the decision (accepted, rejected, or deferred) with the reason.
3. **Specify it.** Accepted language changes become an [OpenSpec](openspec) change: a proposal, a spec with testable scenarios, a design, and a task list.
4. **Build it.** Implementation comes with tests, and with updates to the documentation site.
5. **Archive it.** The spec joins the project's living specification in [`openspec/specs/`](openspec/specs).

The full rules are in [GOVERNANCE.md](GOVERNANCE.md) and [CONTRIBUTING.md](CONTRIBUTING.md).

---

## Getting started

**You need**

- A recent **Rust** toolchain (stable): <https://rustup.rs>
- To compile Brasa programs on Windows: the **Visual Studio C++ Build Tools** (`ml64` and `link`). The compiler finds them on its own.
- Optional: **QEMU** to run Brasa OS (`winget install SoftwareFreedomConservancy.QEMU`).

**Build the compiler**

```sh
git clone https://github.com/code4performance/brasa-parser
cd brasa-parser
cargo build --release
```

The tool is called `brasa-parse` (`target/release/brasa-parse`).

**Use it**

```sh
brasa-parse program.br                 # check a program and print a summary
brasa-parse program.br -o program.exe  # compile to a native executable
brasa-parse program.br --run           # run it with the interpreter (plain programs only)
brasa-parse program.br --asm           # show the generated assembly
brasa-parse program.br --ast           # show the syntax tree
brasa-parse program.br --tokens        # show the tokens
brasa-parse --os os/boot.br os/kernel.br -o brasa-os.img   # build a bootable disk image
brasa-parse --fmt file.br              # format files in the standard style (--fmt-check only lists them)
brasa-parse --docs                     # refresh the examples on the documentation site
```

**Try the examples**

```sh
brasa-parse examples/hello.br -o hello.exe && ./hello.exe
brasa-parse examples/async.br -o async.exe && ./async.exe
.\os\run.ps1                            # build and boot Brasa OS in QEMU
```

**Run the tests**

```sh
cargo test
```

Tests that need the Windows toolchain or QEMU skip themselves when those are missing, so the suite is designed to run anywhere Rust does.

**Read the docs.** Open [`docs/index.html`](docs/index.html) in a browser. It is a static site, no server needed.

---

## What is in the repository

```text
src/
  lexer.rs         tokens, newline rules, text interpolation
  parser.rs        recursive-descent parser for brasa.ebnf
  ast.rs           syntax tree
  interp.rs        tree-walking interpreter (--run)
  codegen_x64.rs   native compiler: type checks, x86-64 code, runtime, linker driver
  asm_x86.rs       the x86 assembler (16, 32 and 64-bit) used for bare-metal images
  cli.rs, docs.rs  command-line options; the documentation generator
std/               standard library, written in Brasa: ui, task, future, fs, net, sys, ws
os/                Brasa OS: boot.br (boot sector) and kernel.br
examples/          example programs
docs/              the documentation site (static HTML)
editors/vscode/    syntax highlighting for .br files
openspec/          proposals, specs and design notes for every language change
tests/             parser, compiler, async, OS and documentation tests
brasa.ebnf         the formal grammar
brasa-ffi.md       C interop and inline assembly specification
```

---

## Join in

Brasa is at the stage where **one contribution can shape the language**. You don't need to be a compiler expert: a clear bug report, a new example, or a better error message all count.

1. Read [CONTRIBUTING.md](CONTRIBUTING.md) for the practical steps.
2. Read [GOVERNANCE.md](GOVERNANCE.md) for the rules, the design principles, and how decisions are made.
3. Everyone in the project follows the [Code of Conduct](CODE_OF_CONDUCT.md).
4. Pick something from the roadmap above, or open an issue and tell us what you'd like to build.

Everything happens on GitHub, so the record is public and searchable:
[issues](https://github.com/code4performance/brasa-parser/issues) for bugs and proposals, and
[pull requests](https://github.com/code4performance/brasa-parser/pulls) for code.

### License

The project is intended to be dual-licensed under **MIT** or **Apache-2.0**, at your choice, and contributions are accepted under the same terms (see [GOVERNANCE.md](GOVERNANCE.md#licensing)). Programs you write *in* Brasa belong entirely to you; the compiler's license does not affect them.

---

*Brasa is built in public. If you believe a programming language can be simple, honest and close to the machine at the same time, come help us prove it.*
