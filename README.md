<p align="center"><img src="docs/assets/zila.svg" alt="Zila Lang" width="96" height="96"></p>

# Zila Lang

**Version 0.1.0** · [Changelog](CHANGELOG.md)

**A small, readable language that compiles straight to machine code.**

Zila Lang is a compiled, statically typed language with readable syntax, where blocks close with the word end. Its compiler, written in Rust with no dependencies, produces x86-64 programs for Windows and Linux itself: no virtual machine, no garbage collector, no runtime to install. Errors are values, memory is reference-counted, and the same language writes applications, C bindings and a kernel.

**What it is for:** a modern language without the overhead: the good features of modern languages (static types, generics, interfaces, errors as values, async and threads), simple to learn, compiled straight to machine code. It was born to work with AI, both as a client (models, tools, MCP, RAG) and as the language that serves a variety of services, with native connections to the main databases (PostgreSQL and SQL Server today) and made for the web, IoT and mobile. It is extensible through packages and open source.

Today it writes command-line tools and servers, native desktop windows, bindings to C libraries and operating-system code. IoT and mobile are targets, not yet built.

Zila Lang is young and honest about what it cannot do yet. If you like compilers and language design, come build it with us: see [CONTRIBUTING.md](CONTRIBUTING.md).

---

## Why Zila Lang?

Most languages ask you to choose: simple *or* fast, safe *or* low-level, friendly *or* close to the machine. Zila Lang's goal is to make that choice smaller.

- **Readable first.** Blocks end with `end`. No braces, no semicolons. A beginner can read a program top to bottom.
- **Honest about failure.** There are no exceptions. A function that can fail says so in its type (`T or error`), and `try` is the one visible place where a failure travels up.
- **Honest about cost.** Memory is freed by reference counting, the moment the last reference goes away. Unsafe code must say `unsafe`. Allocation only happens in known operations (text interpolation, list and map literals, closures that capture, `spawn`, `new`). Two guarantees are built: `#[no_alloc]`, where the compiler proves a function allocates nothing (the allocating operations are a fixed list, and the standard library is checked like your own code), and `#[allocator]`, to replace the allocator on every target (the kernel's heap is written that way). See [what is planned](#how-zila-will-evolve).
- **One language from app to metal.** The same syntax writes an everyday program, a native window, a C library binding, and a kernel.
- **A toolchain you can read.** Lexer, parser, code generator, assembler, standard library: all in this repository, all small enough to understand.
- **Built in the open.** Every language change goes through a written proposal, a spec with testable scenarios, and tests. See [GOVERNANCE.md](GOVERNANCE.md).

---

## A taste of Zila Lang

```zila
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
    let n = try parse("zila")
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
$ zila demo.zila -o demo.exe
$ ./demo.exe
areas: 12 and 12
doubled in parallel: 10 and 12
failed: cannot parse 'bad'
```

A few things to notice: `match` must cover every case of an `enum`; `try` hands a failure to the caller; the two `double` calls run at the same time, so the program waits about 100 ms, not 200; and `await` never blocks the other tasks (it works in any function, not only `async` ones).

---

## The language in pictures

Five diagrams that explain how Zila Lang works. (GitHub draws them for you.)

### 1. From source to machine code

There is no LLVM, no linker and no virtual machine in the middle: the compiler writes the executable itself.

```mermaid
flowchart LR
    SRC["program.zila"] --> LEX["Lexer<br/>(tokens, interpolation)"]
    LEX --> PAR["Parser<br/>(syntax tree)"]
    PAR --> CT{"comptime<br/>generators?"}
    CT -- yes --> GEN["Run generators<br/>(native, cached)"]
    GEN --> PAR
    CT -- no --> CHK["Type check<br/>+ ref / no_alloc checks"]
    CHK --> OPT["Optimizer<br/>(fold, inline, registers)"]
    OPT --> CG["x86-64 code generator"]
    CG --> ASM["Built-in x86 assembler"]
    ASM --> PE["Windows .exe<br/>(PE writer)"]
    ASM --> ELF["Linux ELF"]
    ASM --> IMG["Bootable disk image<br/>(Zila Lang OS)"]
```

### 2. Errors are values

A function that can fail says so in its type. `try` is the one visible place where a failure travels up.

```mermaid
flowchart TD
    CALL["let n = try parse(s)"] --> R{"parse returns<br/>int or error"}
    R -- "ok(v)" --> USE["n = v<br/>execution continues"]
    R -- "err(e)" --> UP["current function returns e<br/>(defers run, memory is freed)"]
    UP --> CALLER["caller receives the error<br/>e.kind, e.message, e.cause"]
    CALLER --> M{"match result"}
    M -- "ok(v)" --> HANDLE1["use the value"]
    M -- "err(e)" --> HANDLE2["handle it, or try again upward"]
```

### 3. Memory without a garbage collector

Values are reference counted and freed the instant the last reference goes away. Writes copy first when a value is shared.

```mermaid
flowchart TD
    A["let a = [1, 2, 3]<br/>count = 1"] --> B["let b = a<br/>count = 2 (cheap, no copy)"]
    B --> W{"write to b?"}
    W -- "yes, still shared" --> COPY["copy on write<br/>b gets its own list, a.count = 1"]
    W -- "no" --> SHARE["a and b share one list"]
    COPY --> END1["scope ends: each count drops"]
    SHARE --> END1
    END1 --> FREE["count = 0: freed immediately<br/>(also on return, break, try)"]
    FREE --> DROP["records with drop()<br/>close files and sockets"]
```

```mermaid
flowchart LR
    P["Parent<br/>new Node"] -- "strong Rc" --> C["Child<br/>new Node"]
    C -. "Weak: weak(parent)" .-> P
    C --> U["w.upgrade()<br/>returns Node? (none if freed)"]
```

A cycle made only of strong `Rc` references is not collected; break it with `Weak`.

### 4. Concurrency: tasks on one scheduler

Tasks are lightweight and cooperative. `await` hands the thread to another task instead of blocking it.

```mermaid
sequenceDiagram
    participant M as main
    participant S as Scheduler (Zila Lang code)
    participant A as double(5)
    participant B as double(6)
    M->>S: let a = double(5)
    M->>S: let b = double(6)
    S->>A: run
    A->>S: await sleep(100)
    S->>B: run
    B->>S: await sleep(100)
    Note over S: both timers wait together<br/>about 100 ms in total, not 200
    S-->>A: timer fired, resume
    S-->>B: timer fired, resume
    M->>S: await a, await b
    S-->>M: 10 and 12
```

### 5. Compile-time code generation

A `comptime func` is a native program that reads a type and writes Zila Lang source, which the compiler compiles in place.

```mermaid
flowchart LR
    T["type User = record<br/>name: text, age: int"] --> ATTR["#[derive(describe)]"]
    ATTR --> REF["Type value<br/>name, kind, fields,<br/>variants, attributes"]
    REF --> GENF["comptime func describe(t: Type)<br/>(checked subset, time and output limits)"]
    GENF --> SRC2["generated Zila Lang source"]
    SRC2 --> BACK["compiled with your code<br/>errors point at your code"]
    GENF -. "result cache on disk" .-> GENF
```

### One language, from app to metal

```mermaid
flowchart TB
    L["Zila Lang<br/>one syntax, one compiler"]
    L --> APP["Everyday programs<br/>text, lists, maps, generics"]
    L --> NET["Concurrent programs<br/>spawn, async/await, net, tls, fs"]
    L --> GUI["Native windows<br/>use ui (Win32 or X11)"]
    L --> FFI["C bindings<br/>extern, #[c_layout], unsafe"]
    L --> OS["Kernel and boot sector<br/>asm, #[naked], no runtime"]
```

---

## What Zila Lang can do today

Everything in this list is implemented and covered by tests.

**A real native compiler**

- Compiles to x86-64 machine code for Windows. A hello-world is about **6 KB**; a native window with a text box, a button and a list is about **50 KB**.
- Compiles to a **static Linux x86-64 ELF** too (`--target linux`), with the same language, the same libraries and the same output. No linker, no C library; a program that calls C becomes a dynamic executable. See [docs/native.html](docs/native.html#linux).
- Needs no runtime on the target machine. The programs import only the operating-system libraries they use.
- Writes the Windows `.exe` itself, with its own assembler and PE writer: building needs no Visual Studio, `ml64` or `link`, just like the Linux ELF path.

**A predictable memory model**

- Reference counting frees every heap value as soon as it is unreachable, including on early `return`, `break` and `try`. There are no GC pauses.
- Records, lists and maps have value semantics: assigning or passing one is cheap (reference counted), and writing to a list or map that another holder still has copies it first (copy on write). `ref` shares the caller's value; to fill a caller's list, take it as `ref xs: []int` and call `fill(ref xs)`. Records can define `drop` to close files and sockets automatically.
- `live_objects()` reports how many heap objects are alive, so leaks are easy to test for. The test suite uses it to check that thousands of tasks and connections leave nothing behind.
- `new Node{ ... }` makes a shared, reference-counted object (`Rc<Node>`); `Weak<T>` with `weak(x)` and `w.upgrade()` holds one without keeping it alive, so trees with parent links, observers and graphs are freed completely. A cycle made only of `Rc` references is still not collected: break it with `Weak`. See `examples/tree.zila`.
- `use text`, `use list`, `use format`, `use set` and `use sorted`: `split`, `join`, `trim`, `replace`, `parse_int`, `parse_float`, `map`, `filter`, `reduce`, `sort_by`, `sort`, `contains` and more, written in Zila Lang on top of generics.
- Generics and interfaces: `func first<T>(items: []T) -> T`, `type Stack<T>`, and `T: Printable` bounds with structural interfaces. The compiler infers the type arguments (or you write them: `empty<int>()`) and makes one copy per type used.
- **Compile-time code generation**: a `comptime func` receives a `Type` (name, kind, fields, variants, attributes) and returns Zila Lang source that the compiler compiles in place. `#[derive(name)]` adds methods to a type, `#[generated(gen)]` writes the body of a function for each type it is called with, and `#[block]` gives a library its own block syntax (a name, raw lines, `end`). Generators are native programs built and run for the build machine, limited to a checked, deterministic subset of the language (no I/O, `spawn`, `unsafe`, `asm` or global variables), with a time and output limit, hygiene for generated names, errors that point at your code, and an on-disk cache. See [docs/syntax.html](docs/syntax.html#comptime) and `examples/comptime.zila`.
- `float` (`f64`) and `f32`: arithmetic, comparisons that follow IEEE 754, explicit `float(n)` / `int(x)` conversions, shortest-text printing, `use math`, and C calls through the XMM registers.
- `Map<K, V>` stores values under any key type that is `Hash` (text, integers, records, enums, lists): `{ "a": 1 }` literals, `m[key]`, `has`, `get(key, default)`, `remove`, and `for key, value in m` in insertion order.
- `Eq`, `Ord` and `Hash` are standard interfaces (no `use`) that the compiler derives for records and enums and you can override with `eq`, `compare` and `hash` methods; `==`, `<` and the other comparison operators work on such types and on a bounded `T: Ord`, and `Self` names the satisfying type inside an interface.
- `defer call(...)` runs a call when the function ends, however it ends (`return`, the end of the body, or a failed `try`), last in first out.

**Types that help**

- Records, enums that carry data, exhaustive `match`, optionals (`T?`, `??`, `?.`), closures, methods, default and named arguments.
- Errors as values with `T or error`, `try`, and `match` on `ok(...)` / `err(...)`. An error has a kind (`e.kind == errors.NotFound`, open and namespaced, `use errors`), a message and a cause: `error(kind, "message", cause)`, `e.context("message")`, `e.is(kind)`, `e.full_message`.

**Concurrency without ceremony**

- Lightweight tasks (`spawn`) and channels, plus `async` / `await` with futures.
- Timers (`sleep`, `timeout`), `await all(...)`, `await race(...)`, file I/O, TCP and UDP networking, asynchronous name lookup and TLS that never block the other tasks, all on one scheduler written in Zila Lang. The same code runs on Windows and on Linux.
- Measured: 2000 tasks at once, and 100 simultaneous TCP clients, each answered correctly.
- Tasks are cooperative and each thread has its own scheduler. For parallelism, `spawn_thread` starts an operating-system thread (Windows), `shared` makes data that threads may read (atomic reference counts, immutable content), and `sync.Mutex<T>` and `sync.Atomic` change it; the compiler decides what may cross to another thread from the types and names the field that cannot. Programs that use no threads pay nothing.

**A standard library written in Zila Lang**

- `task`, `future`, `fs`, `net` (TCP, UDP, DNS), `tls` (an HTTPS client: SChannel on Windows, OpenSSL on Linux), `crypto` (SHA-256, HMAC, PBKDF2, base64, random bytes), `ai` (chat, streaming, typed answers and tools for language models; see docs/ai.html), `mcp` (an MCP server and client over stdio and HTTP), `rag` (chunking, vector stores, retrieval and grounded answers), `db`, `postgres` and `sqlserver` (typed rows, bound parameters, transactions, and SQL written in `sql postgres` and `sql tsql` blocks, written in Zila Lang over `net` and `tls`; see docs/databases.html), `web` (an HTTP/1.1 server with routing and middleware, an HTTP client and HTML templates) and `ui` (windows and controls with one API: native Win32 controls on Windows, controls drawn over the X11 protocol on Linux). On Windows they talk to the operating system through Zila Lang's own `extern` declarations, on Linux through system calls. No Rust or C glue. Of these, `json`, `crypto`, `tls`, `ui`, `web`, `ai`, `mcp`, `rag`, `db`, `postgres` and `sqlserver` are packages of this repository ([`packages/`](packages), embedded by the compiler; a package of your project may take their name and replaces them); the rest is the core, whose names are reserved.

**Systems programming**

- C interop: `extern` functions, callbacks, `#[link("lib")]`, `#[c_layout]`, raw pointers, `unsafe` blocks.
- Inline `asm` with named registers, and `#[naked]` functions.

**Tooling**

- `--fmt` / `--fmt-check`: one standard style, only whitespace changes, comments kept.
- `--test`: runs the `#[test]` functions of a file natively, with `assert` and `assert_eq`.
- `--lsp`: a language server (Language Server Protocol, no dependencies) that reports syntax errors as you type and formats documents.
- A VS Code extension in [`editors/vscode`](editors/vscode): highlighting, syntax errors and Format Document through the language server. No npm dependencies.

**Zila Lang OS**

- A **bootable 64-bit operating system** with its boot sector and kernel written in Zila Lang. The whole disk image is **18 KB**. It boots in QEMU, switches the CPU to long mode, and runs a small shell on the keyboard, the screen and the serial port. The compiler assembles the machine code itself; no NASM, no linker. See [docs/os.html](docs/os.html).

**Engineering you can trust**

- More than 1,000 automated tests (some need Windows, WSL or Node and skip themselves without them). The documentation site is checked against the code by tests, so it cannot quietly go stale. Every design decision lives in [`openspec/`](openspec).

---

## Ideas Zila Lang borrowed

Zila Lang does not pretend to be new in every way. It takes ideas that proved themselves and tries to combine them with less ceremony.

| From | What Zila Lang took | How it looks in Zila Lang |
|---|---|---|
| **Go** | Small language, `package` / `use`, goroutine-style tasks and channels, structural interfaces, one self-contained executable | `spawn worker(ch)`, `channel(int)`, no `implements` keyword |
| **Rust** | Enums with data and exhaustive matching, errors as values, explicit `unsafe`, no tracing garbage collector, a `drop` hook | `match`, `T or error` with `try`, `unsafe ... end`, `func (ref x: T) drop()` |
| **Elixir** | Pattern matching as the main way to take data apart, lightweight tasks, `ok` / `err` results | `ok(v)` / `err(e)` arms, thousands of tasks |
| **Swift** | Reference counting for memory, optionals, string interpolation | `T?`, `??`, `?.`, `"hello {name}"` |
| **C#, JavaScript, Kotlin** | `async` / `await` with futures | `async func`, `await`, `Future<T>` |
| **Ruby, Lua, Pascal** | Blocks closed by `end` | no braces, no semicolons |
| **C and Zig** | A first-class neighbour: C interop, freestanding programs, no hidden runtime | `extern`, `#[c_layout]`, kernels with no operating system |
| **C++** | Operators spelled as words | `bitand`, `bitor`, `bitxor`, `bitnot` |

And a few things that are Zila Lang's own:
- `or error` / `try`: the shape of Rust's `Result` and `?`, in words a beginner can read.
- The whole async scheduler, the networking, the file I/O and the windowing library are **ordinary Zila Lang code**, so you can read and change them.
- One compiler that targets both a Windows program and a bare-metal kernel.

---

## Where Zila Lang stands

Zila Lang is **pre-alpha**. The design is settling, but breaking changes are still possible before 1.0.

| Area | State |
|---|---|
| Lexer and parser | ✅ Complete for the grammar in [`docs/zila.ebnf`](docs/zila.ebnf) |
| Native compiler (Windows x64) | ✅ Works for a large, useful subset. Writes the PE `.exe` itself: no Visual Studio, `ml64` or `link` |
| Bare-metal target and Zila Lang OS | ✅ Boots and runs |
| `async` / `await`, timers, files, TCP, UDP | ✅ Works |
| Floating point | ✅ Works: `float`/`f64`/`f32`, `use math`, C calls, on Windows and Linux |
| Generics and interfaces | ✅ Works: generic functions and records, inferred type arguments, structural interfaces as bounds |
| Maps (`Map<K, V>`) | ✅ Works: any `Hash` key (text, integers, records, enums, lists), insertion order |
| Text, list, format, set and sorted libraries | ✅ `use text`, `use list`, `use format` (`%` templates, padding, hex, thousands, and `format.fmt` for values of any type with `%(08x)`, `%(>10,)` and `%(.2f)`, native only), `use set`, `use sorted` (sorted maps and sets, ordered by `compare` or by a function), `use derive` (`hash_combine`) and `use bufio` (the buffers behind line reading and buffered writing in `fs` and `net`). More to come |
| Text and characters | ✅ A `char` type; `len` counts bytes, `for c in s` walks characters, `slice` cuts by bytes and aborts inside a character, `s.bytes()`, `s.byte(i)`, `s.char_count()`, `text.chars`, `text.from_bytes`; `reverse` and `pad_*` work on characters. **Breaking:** `s[i]` on a text and `chr(n)` are gone (use `s.byte(i)`, `for c in s`, `char(n).to_text()`) |
| Optimizer | 🔧 Constant folding, direct operands, compare-and-branch, peephole cleanups, integer variables in registers and inlining of small functions (`--no-opt` turns them off) |
| Formatter (`--fmt`, `--fmt-check`) | ✅ Standard style for `.zila` files; only whitespace changes, comments kept |
| Test runner (`--test`, `#[test]`, `assert`, `assert_eq`) | ✅ Runs the tests of a file natively and reports `ok`/`FAIL` with the line |
| Language server (`--lsp`) | ✅ Syntax errors and formatting. Hover, go to definition, completion and type errors: ⏳ not started |
| VS Code extension | ✅ Highlighting, syntax errors and formatting through the language server (package it with `npx @vscode/vsce package`; the manual smoke test is still open) |
| TLS (`use tls`) | ✅ HTTPS client on Windows (SChannel) and Linux (OpenSSL 3), with certificate and host-name checks. Server side: ⏳ not started |
| Windows and controls (`use ui`) | ✅ One API on both systems: native on Windows, drawn on X11 on Linux |
| Compile-time reflection (`comptime func`, `#[derive]`, `#[generated]`, blocks) | ✅ Works: the `Type` API, derives, type-indexed functions and block syntax, the checked comptime subset, limits, hygiene, source-mapped errors, a result cache and `--comptime-*` options. Libraries that use it: `json` ✅, `web` ✅ |
| JSON (`use json`) | ✅ `json.parse(T, text)` and `json.encode(T, x)` for any record, enum, list, map, optional, number, `bool` and `text` (derived at build time through reflection: no registration, no JSON in the compiler), `#[json("key")]` and `#[json(skip)]`, the dynamic `json.Value` tree, strict RFC 8259, errors with line and column or a `$.path`, pretty printing, a depth limit; Windows and Linux |
| Web (`use web`) | ✅ An HTTP/1.1 server (one task per connection; `Content-Length` and chunked bodies, keep-alive, size limits, scheduler-driven timeouts, `408`/`413`/`414`/`431`/`503`), routes with `:name` and `*rest`, middleware with `app.use`, errors mapped to statuses by kind, `web.read_json(T, req)` and `web.json(T, status, value)` on `use json`, a client (`web.get`, `web.post`, `web.request`, https through `tls`, redirects, limits) and `html ... end` templates that escape every interpolated value. Windows and Linux; rejected in a bare-metal kernel. Not covered: HTTP/2, WebSocket, TLS on the server side, cookies, multipart, streaming bodies |
| Package manager (`zila.toml`, `zila.lock`, `--init`, `--add`, `--fetch`, `--tidy`, `--update`) | ✅ Dependencies are git repositories pinned by commit and checked by a SHA-256 tree hash; semver with minimal version selection (no solver); majors coexist as different packages; a cache in `~/.zila`; a normal build never touches the network or runs `git`; local packages are directories; `use name.path`; only `pub` items cross packages. No registry, publishing or signing |
| Linux x86-64 (`--target linux`) | ✅ ELF written by the compiler (no linker; static unless the program calls C): the language and every library, `ui` included (drawn on an X11 display), with the same output as Windows (tasks, `async`, files, TCP/UDP, DNS, calling C) |
| macOS, ARM64, Android, iOS | ⏳ Not started |

---

## How Zila Lang will evolve

Every language change is an [OpenSpec](openspec) change. The ones that are written and not yet applied live in `openspec/changes/`; the finished ones are in `openspec/changes/archive/`, and what they established is the living specification in `openspec/specs/`. **Today there are no open changes**: everything the earlier roadmap listed (integer aliases, error kinds, `abort`, `pub`, derived `Eq`/`Ord`/`Hash`, copy on write, weak references, reference escape rules, `#[no_alloc]` and `#[allocator]`, threads, web, SQL blocks, the direct PE writer, the interpreter's removal, the VS Code extension) is built and archived. A test (`tests/docs.rs`) keeps this section from naming a change that is not open.

**Known gaps** (each is a task that its change left behind, or an area with no change yet; every one is a place where you can help):

| Area | What is missing |
|---|---|
| Compiler structure | The checker (`type_of`) types expressions without emitting, but emission still walks the syntax tree, and the cache of checked packages (incremental builds) does not exist yet. |
| Threads | The Linux thread layer (`clone`, `futex`), weak references and identity on `shared T`, a work-stealing scheduler that moves tasks between threads, parallel `async`, a thread pool, condition variables and read-write locks. |
| TLS | TLS for servers, `close_notify` on close, a buffered reader. |
| Language server | Hover, go to definition, completion and type errors. |
| VS Code extension | Packaging the `.vsix` and the manual smoke test. |
| Windows/Linux library parity | `tests/data/std_parity_known.txt` lists the differences between `std/x.zila` and `std/x.linux.zila` that exist today; the list may only shrink. |
| New targets | macOS (a `ui` for it, then the rest), an ARM64 backend for Apple Silicon, Android and iOS, WebAssembly. Not specified yet. |
| Further libraries | Oracle, MySQL and SQLite as packages (a driver and a SQL dialect declaration, no compiler change). |
| Self-hosting | A Zila Lang compiler written in Zila Lang. |

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
- Nothing else to compile Zila Lang programs: the compiler writes the Windows executable itself (no Visual Studio). Building the compiler with Rust's default MSVC toolchain on Windows needs the Visual Studio C++ linker, which is Rust's requirement, not Zila Lang's.
- Optional: **QEMU** to run Zila Lang OS (`winget install SoftwareFreedomConservancy.QEMU`).

**Build the compiler**

```sh
git clone https://github.com/code4performance/zila
cd zila
cargo build --release
```

The tool is called `zila` (`target/release/zila`).

**Use it**

```sh
zila program.zila                 # check a program and print a summary
zila program.zila -o program.exe  # compile to a native Windows executable
zila program.zila --target linux -o program   # compile to a static Linux ELF
zila program.zila --run           # compile and run it
zila program.zila --asm           # show the generated assembly
zila program.zila --ast           # show the syntax tree
zila program.zila --tokens        # show the tokens
zila --os os/boot.zila os/kernel.zila -o zila-os.img   # build a bootable disk image
zila --fmt file.zila              # format files in the standard style (--fmt-check only lists them)
zila --test file.zila             # run the #[test] functions of a file
zila --lsp                      # run the language server on standard input and output
zila --docs                     # refresh the examples on the documentation site
```

**Try the examples**

```sh
zila examples/hello.zila -o hello.exe && ./hello.exe
zila examples/async.zila -o async.exe && ./async.exe
zila examples/hello.zila --target linux -o hello && ./hello   # on Linux (or WSL)
.\os\run.ps1                            # build and boot Zila Lang OS in QEMU
```

**Run the tests**

```sh
cargo test
```

Tests that need the Windows toolchain or QEMU skip themselves when those are missing, so the suite is designed to run anywhere Rust does.

**Use it in VS Code.** The extension in [`editors/vscode`](editors/vscode) gives highlighting, syntax errors as you type and Format Document. Put `zila` on your `PATH`, then `npx @vscode/vsce package` and install the `.vsix`.

**Read the docs.** Open [`docs/index.html`](docs/index.html) in a browser. It is a static site, no server needed.

---

## What is in the repository

```text
src/
  lexer.rs         tokens, newline rules, text interpolation
  parser.rs        recursive-descent parser for zila.ebnf
  ast.rs           syntax tree
  run.rs           --run: build a temporary executable, run it, delete it
  float_text.rs    reference float layout the tests compare native output with
  codegen_x64.rs, codegen_x64/   native compiler: type checks, x86-64 code, runtime, Windows and Linux back ends, split by topic
  prune.rs, effects.rs, sqlblock.rs   reachability pruning, package effects, `sql` blocks
  asm_x86.rs       the x86 assembler (16, 32 and 64-bit): bare-metal images, Windows and Linux programs
  pe.rs            the Windows PE writer
  regalloc.rs      register allocation for the optimizer
  fmt.rs, testrun.rs, lsp.rs   the formatter, the test runner, the language server
  comptime.rs, comptime/   compile-time code generation: reflection data, subset check, generator driver and cache, expansion
  cli.rs, docs.rs  command-line options; the documentation generator
  package.rs, fetch.rs   the package manager: manifest, lock, versions, hashes, cache and resolver; the git runner and the package commands
std/               the core of the standard library, written in Zila Lang: text, list, set, sorted, format, bufio, errors, math, float, fs, net, sync, task, future, thread, sys, reflect, derive
packages/          the other libraries, as packages: json, crypto, tls, ui, web, ai, mcp, rag, db, postgres, sqlserver
tools/             the rename tool and the icon generator
os/                Zila Lang OS: boot.zila (boot sector) and kernel.zila
examples/          example programs
docs/              the documentation site (static HTML), the formal grammar (zila.ebnf) and the C interop and inline assembly spec (zila-ffi.md)
editors/vscode/    VS Code extension: highlighting, syntax errors and formatting through the language server
openspec/          proposals, specs and design notes for every language change
tests/             parser, compiler, async, OS and documentation tests
```

---

## Join in

Zila Lang is at the stage where **one contribution can shape the language**. You don't need to be a compiler expert: a clear bug report, a new example, or a better error message all count.

1. Read [CONTRIBUTING.md](CONTRIBUTING.md) for the practical steps.
2. Read [GOVERNANCE.md](GOVERNANCE.md) for the rules, the design principles, and how decisions are made.
3. Everyone in the project follows the [Code of Conduct](CODE_OF_CONDUCT.md).
4. Pick something from the roadmap above, or open an issue and tell us what you'd like to build.

Everything happens on GitHub, so the record is public and searchable:
[issues](https://github.com/code4performance/zila/issues) for bugs and proposals, and
[pull requests](https://github.com/code4performance/zila/pulls) for code.

### License

The project is intended to be dual-licensed under **MIT** or **Apache-2.0**, at your choice, and contributions are accepted under the same terms (see [GOVERNANCE.md](GOVERNANCE.md#licensing)). Programs you write *in* Zila Lang belong entirely to you; the compiler's license does not affect them.

---

*Zila Lang is built in public. If you believe a programming language can be simple, honest and close to the machine at the same time, come help us prove it.*
