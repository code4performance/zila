# 🔥 Brasa

> **A compiled, statically-typed systems programming language that feels like Go, thinks like Rust, and talks to C without ceremony.**

Brasa compiles directly to native x86-64 machine code — no LLVM, no GC, no runtime overhead. It is designed to be **the language you reach for when you need bare-metal control without sacrificing expressiveness**.

---

## Why Brasa?

| Pain point | C | Rust | Go | **Brasa** |
|---|---|---|---|---|
| Native speed, no GC | ✅ | ✅ | ❌ | ✅ |
| Memory safety | ❌ | ✅ | ✅ | ✅* |
| Ergonomic error handling | ❌ | ✅ | partial | ✅ |
| Bare-metal / no runtime | ✅ | ✅ | ❌ | ✅ |
| Simple learning curve | ❌ | ❌ | ✅ | ✅ |
| Zero-cost C FFI | ✅ | ✅ | ❌ | ✅ |
| Inline assembly | ✅ | ✅ | ❌ | ✅ |
| No LLVM dependency | ✅ | ❌ | ✅ | ✅ |

> *Safety via `T?` / `T!` types, `unsafe` blocks that must be explicit, and no implicit pointer arithmetic. Full memory safety analysis is on the roadmap.

**Brasa's niche:** You want the simplicity of Go, the type safety of Rust, and the raw power of C — all in a single language with a compiler you can read in an afternoon.

---

## Hello, Brasa

```brasa
package main

type Customer = record
    id:    i32
    name:  text
    email: text?        -- optional field
end

func greet(c: Customer) -> text
    return "Hello, {c.name}!"
end

func main()
    var c = Customer{ id: 1, name: "Ana" }
    print(greet(c))

    for i in 1..5
        if i % 2 == 0
            print("{i} is even")
        end
    end
end
```

```
$ brasa-parse hello.br --build -o hello
$ ./hello
Hello, Ana!
2 is even
4 is even
```

---

## Features

### From **Go** — simplicity and structure
- `package` / `use` module system
- Structural interfaces (implement implicitly, no `implements` keyword)
- `defer` for deterministic cleanup
- `spawn` for concurrent goroutine-style tasks
- Block syntax with `end` (no braces, no semicolons)
- Named return parameters

```brasa
type Writer = interface
    func write(ref self, data: []u8) -> i32 or error
end

func copy(src: Reader, dst: Writer)
    defer dst.flush()
    for chunk in src.chunks(4096)
        try dst.write(chunk)
    end
end
```

### From **Rust** — type safety without the ceremony
- `T?` optionals (instead of `Option<T>`)
- `T or error` fallible returns (instead of `Result<T, E>`)
- `try` / `?` error propagation
- Generics with interface bounds: `func show<T: Printable>(x: T)`
- `match` with exhaustive patterns and `|` alternation
- `unsafe` blocks that make unsafety explicit

```brasa
func parse_int(s: text) -> i32 or error
    let n = try std.parse_i32(s)   -- propagates error automatically
    return n
end

func classify(n: i32) -> text
    match n
        0           -> return "zero"
        1 | 2 | 3   -> return "small"
        _           -> return "other"
    end
end
```

### From **C / Zig** — zero-cost interop and raw power
- Zero-overhead `extern "C"` function declarations
- Variadic C functions (`printf`, `qsort`, etc.)
- `#[c_layout]` for C-compatible record layouts
- `#[link("libname")]` for library linking
- `#[link_src("file.c")]` to compile C files alongside Brasa
- Inline assembly with named register bindings (`in`, `out`, `inout`, `clobber`)
- `#[naked]` for hand-written prologue/epilogue
- Raw pointer types (`*T`, `*c.void`)
- `#[export("symbol")]` to expose Brasa functions to C

```brasa
#[link("m")]
extern "C" func sqrt(x: f64) -> f64

extern "C" func printf(fmt: *c.char, ...) -> c.int

#[c_layout]
type Point = record
    x: f64
    y: f64
end

#[cfg("x86_64")]
func rdtsc() -> u64
    var lo: u32 = 0
    var hi: u32 = 0
    unsafe
        asm
            "rdtsc"
            out eax = lo
            out edx = hi
        end
    end
    return u64(hi) << 32 bitor u64(lo)
end
```

### From **Swift / Kotlin** — ergonomic strings
- String interpolation: `"Hello, {name}! You have {count} messages."`
- Multi-line strings
- `text` as a first-class string slice (UTF-8, no null terminator)

### From **functional languages** — expressive types
- Tagged union types (`enum` with payloads)
- Structural pattern matching via `match`
- `|` alternation in patterns
- Closures / first-class functions

```brasa
type Shape = enum
    Circle(float)
    Rect(float, float)
    Empty
end

func area(s: Shape) -> float
    match s
        Circle(r)  -> return 3.14159 * r * r
        Rect(w, h) -> return w * h
        Empty      -> return 0.0
    end
end
```

### Original to **Brasa**
- `bitand` / `bitor` / `bitxor` / `bitnot` as keywords (not symbols) — readable assembly-level code
- `ref` parameter modifier for pass-by-reference (explicit, like `&mut` but simpler)
- `text` type — a UTF-8 string slice, the default string type (not a `String` heap object)
- `nil` for optional absence (not `null`, not `None`)
- `??` nil-coalescing operator and `?.` optional chaining

---

## Quick Start

### Prerequisites

- **Rust toolchain** (stable, 1.70+): https://rustup.rs
- **MSVC Build Tools** (Windows) — includes `ml64.exe` and `link.exe`
- Linux/macOS backends are on the roadmap

### Build from source

```sh
git clone https://github.com/brasa-lang/brasa-parser
cd brasa-parser
cargo build --release
# Binary is at: target/release/brasa-parse
```

### Compile a Brasa program

```sh
# Parse and validate
brasa-parse hello.br

# Compile to native executable
brasa-parse hello.br --build -o hello.exe

# Dump x86-64 assembly
brasa-parse hello.br --asm

# Run via AST interpreter (no compile step)
brasa-parse hello.br --run

# Inspect the AST
brasa-parse hello.br --ast

# Inspect tokens
brasa-parse hello.br --tokens
```

### VS Code / Antigravity syntax highlighting

The `editors/vscode/` directory contains a ready-to-install extension for `.br` files:

```sh
# VS Code
cp -r editors/vscode ~/.vscode/extensions/brasa-lang

# Antigravity IDE
cp -r editors/vscode ~/.antigravity/extensions/brasa-lang
```

---

## More Examples

### Error handling and optionals

```brasa
package main

use file, json

func load(path: text) -> Config or error
    let raw = try file.read(path)
    let f   = try file.open(path)
    defer f.close()
    return try json.parse(Config, raw)
end

func main() -> error
    let email: text? = c.email
    let display = email ?? "no email"       -- nil-coalescing
    let len     = email?.len                -- optional chaining

    match load("config.json")
        ok(cfg) -> use_config(cfg)
        err(e)  -> print("failed: {e.message}")
    end
end
```

### REST API server

```brasa
package main

use http, db, json

type Customer = record
    id:   i32
    name: text
end

func main() -> error
    let conn = try db.open("postgres://localhost/shop")
    defer conn.close()

    var app = http.server(port: 8080)

    app.get("/customers", func(req)
        let rows = try conn.query(Customer, "select id, name from customers")
        return http.json(rows)
    end)

    app.post("/customers", func(req)
        let c = try req.body(Customer)
        try conn.exec("insert into customers(name) values($1)", c.name)
        return http.created(c)
    end)

    app.run()
end
```

### Inline assembly and C interop

```brasa
package main

use c

extern "C" func printf(fmt: *c.char, ...) -> c.int

#[naked]
#[cfg("x86_64")]
func halt()
    asm
        "cli"
        "hlt"
    end
end

func main()
    unsafe
        printf(c.cstr("sqrt(2) = %f\n"), sqrt(2.0))
    end
    print("tsc: {rdtsc()}")
end
```

---

## Architecture

```
Source (.br)
     │
     ▼
  Lexer         — keywords, operators, interpolated strings, numbers, comments
     │                 recognises line-breaks as statement terminators
     ▼
  Parser         — recursive-descent, full grammar (brasa.ebnf)
     │                 produces a typed AST
     ▼
   AST
  ╱   ╲
 ▼     ▼
Interp  Codegen x86-64
(--run)       │
              ▼
         MASM (.asm)
              │
         ml64 /nologo
              │
         link.exe
              │
         Native PE/ELF binary   (~10 KB for hello world)
```

**Zero external Cargo dependencies.** The entire compiler is `std`-only Rust.  
**No LLVM.** Code generation emits MASM-syntax x86-64 assembly directly, then invokes `ml64` and `link.exe`. The resulting binary has no GC, no runtime, no libc dependency by default (pure `kernel32.lib` path produces binaries as small as **2.5 KB**).

---

## Repository Layout

```
brasa-parser/
├── src/
│   ├── lexer.rs         -- tokeniser (keywords, literals, interpolation)
│   ├── ast.rs           -- AST node types
│   ├── parser.rs        -- recursive-descent parser
│   ├── interp.rs        -- tree-walk interpreter (--run)
│   ├── codegen_x64.rs   -- x86-64 MASM code generator (--build)
│   ├── asm_x86.rs       -- low-level assembly emission helpers
│   ├── cli.rs           -- command-line interface
│   ├── docs.rs          -- built-in documentation strings
│   ├── lib.rs           -- library exports
│   └── main.rs          -- binary entry point
├── examples/
│   ├── hello.br         -- greetings and loops
│   ├── types.br         -- records, enums, generics, pattern matching
│   ├── errors.br        -- T?, T or error, try, ??, ?., defer, spawn
│   ├── rest_api.br      -- HTTP server + PostgreSQL
│   ├── ffi.br           -- extern "C", variadic, inline asm, #[naked]
│   ├── lowlevel.br      -- raw pointers, unsafe, bit ops
│   ├── async.br         -- spawn, channels
│   ├── control.br       -- for, while, match, if-else chains
│   └── ui.br            -- UI / layout concepts
├── editors/
│   └── vscode/          -- VS Code / Antigravity syntax extension
├── std/                 -- standard library sketches (WIP)
├── docs/                -- language design documents
├── tests/               -- integration test suite (29 tests)
├── brasa.ebnf           -- formal grammar (EBNF)
├── brasa-ffi.md         -- C/Assembly interop specification
└── Cargo.toml           -- zero external dependencies
```

---

## Roadmap

### v0.2 — Semantic Analysis
- [ ] Name resolution and scope tracking
- [ ] Type inference engine
- [ ] Exhaustive `match` checking
- [ ] Duplicate declaration errors
- [ ] Type-checked generics (monomorphisation)

### v0.3 — Standard Library
- [ ] `io` — file I/O, stdin/stdout, buffered readers/writers
- [ ] `collections` — List, Map, Set, Queue, Stack
- [ ] `text` — string formatting, parsing, regex
- [ ] `net` / `http` — TCP, HTTP/1.1, HTTP/2
- [ ] `json` / `yaml` / `toml` — serde-style codecs
- [ ] `db` — database connection interface + PostgreSQL driver

### v0.4 — Linux / macOS Backends
- [ ] ELF backend (Linux x86-64 and ARM64)
- [ ] Mach-O backend (macOS ARM64 / Apple Silicon)
- [ ] System V AMD64 ABI calling convention
- [ ] Cross-compilation support

### v0.5 — Language Server & Tooling
- [ ] LSP server (`brasa-lsp`) for IDE completion, hover, go-to-def
- [ ] Formatter (`brasa-fmt`)
- [ ] Package manager (`brasa-pkg`) with central registry
- [ ] Build system (`brasa.toml` project manifest)

### v0.6 — Advanced Language Features
- [ ] `html ... end` template blocks (markup-aware lexer mode)
- [ ] Explicit generic call syntax (`first<i32>(list)`)
- [ ] Error recovery: collect multiple parse errors before stopping
- [ ] Compile-time evaluation (`const func`)
- [ ] `async` / `await` on top of `spawn` primitives
- [ ] Lifetime annotations (opt-in, for systems code)

### v1.0 — Self-hosting
- [ ] Brasa compiler written in Brasa
- [ ] Brasa OS kernel demo (bare-metal boot)
- [ ] WebAssembly backend

> Items are roughly ordered by priority. Community input shapes what gets done first — open an issue or start a discussion!

---

## Contributing

Brasa is at an early, exciting stage where **every contribution has outsized impact**. Whether you're a compiler engineer, a language designer, a documentation writer, or someone who just wants to try writing `.br` files and report what breaks — you are welcome here.

### Areas that need contributors right now

| Area | Skills needed | Difficulty |
|---|---|---|
| Semantic analysis (type checker) | Compiler theory, Rust | ⭐⭐⭐ |
| Linux ELF backend | x86-64 ABI, ELF format, Rust | ⭐⭐⭐ |
| Standard library modules | Systems programming, Rust | ⭐⭐ |
| LSP server | LSP protocol, Rust | ⭐⭐⭐ |
| VS Code extension (richer) | TypeScript, TextMate grammars | ⭐⭐ |
| More `.br` examples | Brasa language, any domain | ⭐ |
| Documentation / tutorials | Technical writing | ⭐ |
| Bug reports & test cases | Using the language | ⭐ |
| Package manager | CLI tooling, Rust | ⭐⭐⭐ |

---

## Contributor Charter

*The rules every participant in the Brasa project agrees to follow.*

### 1. Code of Conduct

Everyone — contributors, maintainers, and community members — is expected to:

- **Be respectful.** Critique ideas, not people. Disagreement is healthy; personal attacks are not.
- **Be inclusive.** Brasa welcomes contributors regardless of background, experience level, nationality, gender, or identity.
- **Be constructive.** When something is wrong, explain why and suggest a fix.
- **Be patient.** Maintainers are volunteers. Response times vary.

Violations may result in warnings, temporary bans, or permanent removal from the community at maintainer discretion.

### 2. How to Propose Changes

#### Bug fixes and small improvements
Open a Pull Request directly. Fixes, test additions, documentation improvements, and typos do not need a prior issue.

#### New language features
Brasa follows an **RFC (Request for Comments) process** for language changes:

1. **Open a Discussion** in GitHub Discussions, titled `[RFC] Your Proposal`.
2. Describe: *what*, *why*, *how it fits the language philosophy*, and *alternatives considered*.
3. The community debates and refines the proposal for a minimum of **14 days**.
4. A maintainer closes the RFC as **accepted**, **rejected**, or **deferred**.
5. Only after acceptance may a PR implementing the feature be opened.

Language changes that bypass the RFC process will not be merged.

#### Compiler and tooling changes
Changes to the parser, codegen, or stdlib architecture should include a brief design note in the PR description explaining the approach and trade-offs.

### 3. Pull Request Rules

All pull requests must:

- **Pass all existing tests.** Run `cargo test` before submitting. PRs that break tests will not be reviewed until fixed.
- **Include tests.** New features and bug fixes must come with test cases in `tests/`.
- **Be written in Rust or Brasa.** The compiler is Rust; stdlib and examples are Brasa. No other languages in the core repository without an RFC.
- **Follow the zero-dependency rule.** The core compiler (`brasa-parser`) must have zero external Cargo dependencies. Tooling (`brasa-lsp`, `brasa-fmt`) may have dependencies if justified.
- **Not introduce breaking changes without an RFC.** Backwards-incompatible grammar or semantic changes require RFC approval.
- **Have clear commit messages.** Format: `component: short description` (e.g., `parser: support explicit generic call syntax`).
- **Be rebased onto `main`.** No merge commits in feature branches.

### 4. Language Design Principles

These principles guide every design decision. When in doubt, refer to them:

1. **Explicit over implicit.** If something is happening (a copy, a pointer, an unsafe operation), it should be visible in the code.
2. **Simple over clever.** Prefer straightforward constructs over syntactic magic. If a feature needs a manual chapter to explain, it probably doesn't belong.
3. **No implicit conversions.** `i32` and `i64` do not coerce. `text` and `*c.char` do not coerce. The programmer decides.
4. **Errors are values.** Brasa has no exceptions. Errors flow through `T or error` return types and are propagated with `try`.
5. **Unsafe must be loud.** Code that could invoke undefined behaviour must be inside an `unsafe` block. Unsafe blocks should be small and commented.
6. **Keep the compiler lean.** The compiler itself must have zero external Cargo dependencies. Speed of compilation matters.
7. **C is a first-class neighbour.** Brasa must be able to call C with zero overhead and zero friction. The world's C libraries are an asset, not a legacy.
8. **Batteries optional.** The language is small; the standard library grows incrementally. External packages fill the gaps.

### 5. Governance

Brasa currently operates under a **Benevolent Dictator** model: the project founder holds final say on language design and architectural decisions.

As the project grows, governance will evolve:

- **v0.3+:** A **Language Design Committee** (3–5 members) with RFC voting rights will be formed from active contributors.
- **v1.0:** Fully open governance with a written constitution, committee elections, and a public roadmap vote.

If you want a seat at the table, the best path is sustained, high-quality contribution.

### 6. Licensing

Brasa is dual-licensed under:

- **MIT License** — [`LICENSE-MIT`](LICENSE-MIT)
- **Apache License 2.0** — [`LICENSE-APACHE`](LICENSE-APACHE)

You may use Brasa under either license. Contributions are accepted under the same dual license. By submitting a Pull Request you certify that you have the right to submit your contribution under these terms.

Programs written *in* Brasa are entirely yours — the compiler license does not affect the programs it compiles.

---

## Language Design at a Glance

Brasa borrows the best ideas from each language and leaves the complexity behind:

| Feature | Inspired by | What Brasa does differently |
|---|---|---|
| Package system | Go | Same `package` / `use` model |
| `defer` | Go | Identical semantics |
| `spawn` | Go `go` | Same goroutine-style, channels built-in |
| Structural interfaces | Go | No `implements` keyword needed |
| `T?` optional | Rust `Option<T>` | Shorter syntax, `nil` for absence |
| `T or error` fallible | Rust `Result<T,E>` | `try` keyword instead of `?` suffix |
| Generics with bounds | Rust traits | `<T: Interface>` — simpler bounds |
| `match` patterns | Rust | `|` alternation, multi-line arms |
| `unsafe` blocks | Rust / C | Explicit, no implicit unsafety |
| `extern "C"` FFI | Rust / Zig | Zero overhead, variadic C functions |
| Inline `asm` | Rust / GCC | Named bindings: `in`, `out`, `inout`, `clobber` |
| `#[naked]` | Rust | Same attribute, hand-written asm functions |
| `#[c_layout]` | Zig `extern struct` | C-compatible record layout |
| String interpolation | Swift / Kotlin | `"Hello, {expr}"` |
| `bitand/bitor/bitxor` | Ada / SQL | Words, not symbols — readable in systems code |
| `ref` modifier | C++ `&` / Rust `&mut` | Explicit pass-by-reference, no borrow checker |
| `text` type | Go `string` | UTF-8 slice, immutable by default |
| Tagged union `enum` | Rust | With payloads, destructured in `match` |

---

## Community

- **GitHub Issues** — bug reports, feature requests
- **GitHub Discussions** — RFCs, design questions, show & tell
- **GitHub Pull Requests** — code contributions

There is no Discord or Slack yet. Everything happens on GitHub so the record is permanent and searchable.

---

## Status

> Brasa is **pre-alpha** software. The language design is stabilising but breaking changes are expected before v1.0.

| Component | Status |
|---|---|
| Lexer | ✅ Complete |
| Parser | ✅ Complete (29/29 tests) |
| AST Interpreter (`--run`) | ✅ Working |
| x86-64 Code Generator | 🔧 Working, partial coverage |
| Semantic Analysis | ⏳ Not started |
| Standard Library | 🔧 Sketches only |
| Linux Backend | ⏳ Not started |
| Package Manager | ⏳ Not started |
| LSP Server | ⏳ Not started |

---

*Brasa is built in public. If you believe that systems programming deserves better ergonomics — without sacrificing performance or control — come build it with us.*

**[⭐ Star the repo](https://github.com/brasa-lang/brasa-parser) · [🐛 Report a bug](https://github.com/brasa-lang/brasa-parser/issues) · [💡 Start an RFC](https://github.com/brasa-lang/brasa-parser/discussions)**
