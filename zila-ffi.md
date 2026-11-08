# Zila Lang FFI & Inline Assembly Specification

This document defines the Foreign Function Interface (FFI) and Inline Assembly specifications for the Zila Lang programming language.

Zila Lang is designed to combine modern expressiveness and safety with seamless, zero-cost systems-level interoperability. It can call into C libraries, be called from C runtimes, and execute target-specific inline assembly without runtime glue or foreign bridges.

---

## 1. Safety and Unsafe Boundaries

Zila Lang divides operations into safe code and explicitly audited operations:
- Calling external foreign functions declared via `extern` is considered potentially unsafe.
- Dereferencing raw pointers (`ptr^`) is unsafe.
- Executing inline assembly (`asm ... end`) is unsafe.

All such operations require an enclosing `unsafe` block:

```zila
unsafe
    printf(c.cstr("Result: %d\n"), calculate())
end
```

The `unsafe` block signals to both the programmer and compiler that memory safety and type soundness guarantees are delegated to manual inspection.

---

## 2. Calling C from Zila Lang (`extern`)

### 2.1 Foreign Function Declarations

External functions are declared using the `extern` keyword. The calling convention (ABI) is optional and defaults to `"C"`:

```zila
// Defaults to ABI "C"
extern func abs(x: c.int) -> c.int

// Explicit ABI
extern "C" func sqrt(x: f64) -> f64
```

Supported ABIs:
- `"C"` (default): Standard C calling convention for the target platform (System V AMD64 ABI on Unix, Microsoft x64 on Windows, AAPCS on ARM).
- `"stdcall"`: Standard Windows 32-bit API convention.
- `"system"`: Native system call or platform default convention.
- `"fastcall"`: Register-passing calling convention where supported.

### 2.2 C Variadic Functions

C variadic functions (like `printf`) are declared with trailing `...` in the parameter list:

```zila
extern "C" func printf(fmt: *c.char, ...) -> c.int
```

When calling a variadic function, arguments corresponding to `...` are converted according to standard C default argument promotions (e.g., small integers promoted to `c.int`, `f32` promoted to `f64`).

---

## 3. The `c` Package & Type Mappings

Zila Lang provides a standard library package `use c` with definitions matching the host platform's C toolchain.

### 3.1 Primitive Types

| Zila Lang Type | C Equivalent | Notes |
|---|---|---|
| `c.char` | `char` | Target-dependent sign (usually `i8`) |
| `c.schar` | `signed char` | Signed 8-bit integer |
| `c.uchar` | `unsigned char` | Unsigned 8-bit integer |
| `c.short` | `short` | Signed 16-bit integer |
| `c.ushort` | `unsigned short` | Unsigned 16-bit integer |
| `c.int` | `int` | Typically 32-bit signed integer |
| `c.uint` | `unsigned int` | Typically 32-bit unsigned integer |
| `c.long` | `long` | 32-bit on Windows x64, 64-bit on POSIX x86-64 |
| `c.ulong` | `unsigned long` | Unsigned long |
| `c.longlong` | `long long` | 64-bit signed integer |
| `c.ulonglong` | `unsigned long long` | 64-bit unsigned integer |
| `c.size_t` | `size_t` | Pointer-sized unsigned integer (`usize`) |
| `c.ssize_t` | `ssize_t` | Pointer-sized signed integer (`isize`) |
| `c.float` | `float` | 32-bit IEEE 754 float (`f32`) |
| `c.double` | `double` | 64-bit IEEE 754 float (`f64`) |
| `c.void` | `void` | Used exclusively behind pointers (`*c.void`) |

### 3.2 Pointers

- Raw pointer to type `T`: `*T` (e.g. `*u8`, `*c.char`, `*c.void`).
- Address-of: `&x` produces `*T` from a place expression.
- Dereference: `ptr^` reads or writes the memory pointed to by `ptr`.
- Null pointers: Represented as `nil` when interacting with pointer types, or typed explicitly as `(*T)?` for optional pointers.

### 3.3 Text & String Interoperability

Zila Lang `text` is a slice of UTF-8 bytes (pointer + length) without a trailing null terminator. C expects null-terminated pointers (`*c.char`). The `c` module provides conversion utilities:

- `c.cstr(str: text) -> *c.char`: Produces a temporary null-terminated C string valid for the lifetime of the surrounding expression or function call.
- `c.from_cstr(ptr: *c.char) -> text`: Computes the string length via `strlen` and returns an immutable Zila Lang `text` view.

---

## 4. Being Called from C & Callbacks

### 4.1 Exporting Functions

Functions defined in Zila Lang can be exported to C using the `#[export]` attribute:

```zila
#[export("zila_add")]
func add(a: i32, b: i32) -> i32
    return a + b
end
```

The compiler exports the symbol with the specified unmangled name and standard C calling convention.

### 4.2 Function Pointers and Callbacks

C APIs frequently require function pointers as callbacks (e.g., comparator functions in `qsort`). Zila Lang supports `extern [abi] func(...)` as a first-class type:

```zila
extern "C" func qsort(
    base: *c.void,
    n: c.size_t,
    size: c.size_t,
    cmp: extern "C" func(*c.void, *c.void) -> c.int
)
```

A top-level Zila Lang function matching the signature and declared with `extern "C"` or `#[export]` can be passed directly as this parameter.

---

## 5. Struct Layout & Data Alignment

By default, Zila Lang may reorder record fields to minimize padding and memory footprint. When interfacing with C, field order and alignment must match C compiler rules precisely:

```zila
#[c_layout]
type Point = record
    x: f64
    y: f64
end
```

The `#[c_layout]` attribute guarantees:
1. Field ordering matches the source declaration order exactly.
2. Alignment and padding adhere to the target ABI rules (equivalent to C `struct`).
3. The record can be cast to and from raw pointers (`*Point`) or passed across FFI boundaries by reference or value.

---

## 6. Linking Directives

Zila Lang source files can specify dependencies directly via top-level attributes:

- `#[link("libname")]`: Instructs the linker to link against a system or installed library (e.g., `#[link("m")]` links `libm`, `#[link("user32")]` links `user32.lib`).
- `#[link_src("path/to/file.c")]`: Instructs the compiler to compile and link a local C source file directly alongside the Zila Lang package. Useful for vendored C dependencies.
- `#[cfg("predicate")]`: Conditional compilation directive. Restricts declarations to matching architectures or operating systems (e.g., `#[cfg("x86_64")]`, `#[cfg("windows")]`, `#[cfg("linux")]`).

---

## 7. Inline Assembly (`asm ... end`)

Zila Lang supports inline assembly for architecture-specific instructions, hardware access, performance-critical routines, and OS kernels.

### 7.1 Syntax

```zila
unsafe
    asm
        <instruction template 1>
        <instruction template 2>
        in operand_name = expr
        out operand_name = var_name
        inout operand_name = var_name
        clobber reg1, reg2
    end
end
```

### 7.2 Instructions and Operand Substitution

- Each instruction line is target assembly code written as it is, without quotes (everything up to a `//` comment is passed to the assembler; `;` comments, labels such as `start:` and quoted data such as `db "text", 0` work as in any assembler, and instructions named like Zila Lang words, e.g. `in al, dx`, are fine). A line starting with `in`/`out`/`inout`, a name and `=` is an operand instead, and the block ends at a line that is just `end`.
- Lines written as string literals (`"cli"`) are still accepted.
- Operands are referenced using the `@name` syntax within instruction templates.
- When an operand name matches a known hardware register (e.g., `eax`, `edx`, `rax`), the compiler binds that operand to the specified physical register.
- When an operand name is a general identifier, the compiler allocates an appropriate general-purpose register.

### 7.3 Direction Specifiers

1. `in name = expr`: Input operand. Passes the evaluated expression into a register or memory location. Read-only for the assembly block.
2. `out name = var`: Output operand. Assigns the result from the register to the given mutable variable upon block exit.
3. `inout name = var`: Read-write operand. Supplies an initial value and writes back the modified register contents to the variable.

### 7.4 Clobber Lists

The `clobber` clause lists registers and state flags mutated by the assembly code that are not bound to output operands:
- Register names: e.g. `"rax"`, `"rcx"`, `"xmm0"`.
- Condition codes: `"cc"` notifies the compiler that arithmetic status flags were altered.
- Memory barrier: `"memory"` indicates that memory was read or written arbitrarily, forcing the compiler to flush and reload memory caches across the assembly boundary.

### 7.5 Naked Functions

For interrupt service routines, context-switching routines, and boot code, the `#[naked]` attribute omits compiler-generated function prologues and epilogues (no stack frame setup or return instructions generated):

```zila
#[naked]
#[cfg("x86_64")]
func halt()
    asm
        cli
        hlt
    end
end
```

---

## 8. Comprehensive Example

```zila
package main

use c

// External standard library math function
#[link("m")]
extern "C" func sqrt(x: f64) -> f64

// External variadic I/O
extern "C" func printf(fmt: *c.char, ...) -> c.int

// Struct matching C ABI
#[c_layout]
type Vector2D = record
    x: f64
    y: f64
end

// Exported function callable from C
#[export("vector_length")]
func vector_length(ref v: Vector2D) -> f64
    return sqrt(v.x * v.x + v.y * v.y)
end

// High-resolution CPU timestamp counter via inline asm
#[cfg("x86_64")]
func rdtsc() -> u64
    var lo: u32 = 0
    var hi: u32 = 0
    unsafe
        asm
            rdtsc
            out eax = lo
            out edx = hi
        end
    end
    return u64(hi) << 32 bitor u64(lo)
end

func main()
    var v: Vector2D = Vector2D { x: 3.0, y: 4.0 }
    let len: f64 = vector_length(ref v)

    unsafe
        printf(c.cstr("Length: %f\n"), len)
    end

    print("Timestamp: {rdtsc()}")
end
```

