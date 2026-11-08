## Context

The standard library is a set of Zila Lang modules embedded in the compiler (`std_source` in `src/codegen_x64.rs`: `task`, `future`, `fs`, `net`, `sys`, `ui`, `ws`, `float`, `math`), loaded when a program says `use name`. Text has `len`, `slice(a, b)`, byte indexing `s[i]`, `+`, `==` and the built-in `chr(n)`; lists have `add`, `take`, `clear`, `len` and indexing. Generics (with inference and structural bounds) and floats now exist, so most of the library can be written in Zila Lang itself. The interpreter behind `--run` has no module system today (it ignores `use`).

## Goals / Non-Goals

**Goals**
- Almost all of the library in Zila Lang, tested natively and by the interpreter, with the compiler changed only where the language lacks a primitive.
- Predictable, documented byte semantics.

**Non-Goals**
- Regular expressions, Unicode algorithms, a formatting language, new collection types, I/O.

## Decisions

1. **Two modules, `text` and `list`.** Functions, not methods, so `text.trim(s)` never collides with user code and the generic `list` helpers are ordinary generic functions (`func map<T, U>(xs: []T, f: func(T) -> U) -> []U`).
2. **Text functions are byte algorithms in Zila Lang.** `index_of` is a naive scan comparing bytes (`s[i] == part[j]`); `split`, `replace` and `contains` are built on it. Naive search is O(n*m), fine for the sizes Zila Lang programs handle now; a faster search can replace it without changing behavior. Results are built with `slice` and `+`; to avoid quadratic concatenation, `join`, `replace`, `repeat` and `pad_*` collect pieces in a list and concatenate once with a small private `concat_all(parts)` helper that sizes the result using a byte buffer if the language offers one, and otherwise concatenates pairwise in a balanced way (O(n log n)). A runtime helper is added only if measurement shows it is needed (decision recorded in the task).
3. **Case and reverse need byte-to-text.** They build texts from bytes with `chr(b)` (`chr` produces a one-byte text for values below 128 and the byte sequence otherwise, as today). To keep non-ASCII bytes intact, the functions copy runs between changed bytes with `slice` and only call `chr` for the converted ASCII letters.
4. **Parsing.** `parse_int` accumulates into a negative number so `-9223372036854775808` parses, and checks overflow before each step. `parse_float` splits the text into sign, integer digits, fraction digits and exponent, and builds the value with exact integer arithmetic where it is exact (up to 15 significant digits and a power of ten up to 22, the fast exact path every correctly-rounded parser starts with), and otherwise falls back to a correctly rounded slow path: scale the decimal into a big integer ratio and use the big-number routines already in `std/float.br` (moved to a shared internal module `bignum`, imported by both). This makes `parse_float(print(x)) == x` hold for every float, which the tests verify over thousands of bit patterns.
5. **Errors as values.** Parsers return `T or error`; the messages quote the text (`not a whole number: "12x"`). `split` with an empty separator stops the program with a panic, like a bad index, because it is a programming mistake, not input.
6. **List functions are generic and allocate.** Each builds a fresh `[]U` with `add`. Lists created with `let out: []U = []` get the right element-ownership flag from the element type, so reference counting needs no new code. `sort_by` is a top-down stable merge sort over a copy using a scratch list, so inputs are untouched; `less` is called as `less(b, a)` only to decide whether to take from the right half, which gives stability.
7. **Lambdas and inference.** Calls like `list.map(xs, func(n) n * 2 end)` need the lambda's parameter types and the result type. Inference already binds `T` from `xs`, then types the lambda from `func(T) -> U`, but `U` appears only in the lambda's result; the call site must infer `U` from the lambda's body. This is an addition to `call_generic`: after the other arguments are bound, compile each lambda argument with its parameter types known, take the type of its result, and unify the declared function result type with it. If that is not enough for a case, the task records the limitation and the docs show the explicit form (a named function).
8. **Interpreter modules.** The interpreter gets a minimal module mechanism: for each `use name` it parses the embedded std source for `text` and `list`, loads its functions under the module name, and resolves `name.func(...)` calls on an identifier that names a loaded module. Generic functions run as they are (the interpreter ignores types). `parse_int` and `parse_float` need `or error` results: the interpreter already models errors for `try` and `??` in its own way, so these functions use plain Zila Lang code that builds errors with `error("...")`.
9. **Docs.** One new page `docs/stdlib.html` documents both modules with a table per function; every page's navigation links to it (the docs tests require shared navigation and resolving links).

## Risks / Trade-offs

- **Quadratic text building** if `+` in a loop is used naively. Mitigation: the balanced concatenation helper, plus a test that joins 100000 pieces in reasonable time.
- **Correctly rounded `parse_float`** is the riskiest piece. Mitigation: reuse the big-number code, and test against Rust's parser on thousands of random decimal strings and bit patterns.
- **Inferring a lambda's result type** (decision 7) touches the compiler. Mitigation: do it in its own task with focused tests, and fall back to documenting named functions if it proves too invasive.
- **Interpreter modules** are new machinery. Mitigation: keep it minimal and covered by the same-output tests; any std feature the interpreter lacks is listed in the task that finds it.
- **ASCII-only case mapping** surprises some users. Mitigation: documented clearly, with the names `upper` and `lower` explained as ASCII on the docs page.

## Changes made while building (recorded after the fact)

- **Compiler additions beyond registering the modules:** `panic("message")` (a built-in that stops the program like an index error, needed for `split needs a separator`), the intrinsics `__from_bits` and `__bits` (reinterpret an `int` as a `float` and back, needed to build the result of `parse_float`), and inference of a lambda argument's result type for generic calls, with an error when it would be `nothing`.
- **Big numbers are not moved into a new module.** `text` does `use float` and calls the existing big-number functions as `float.big_from`, `float.b_mul` and so on. A consequence is that `use text` also links the float printing library.
- **`text.compare(a, b)`** was added because text has no `<` operator yet; `sort_by` on texts uses `text.compare(a, b) < 0`.
- **Interpreter:** `use text`, `use list` (and the `float` and `math` modules they need) load from the embedded sources into per-module namespaces; text indexing now gives the byte (as the native compiler does); lambdas return their last expression; `error`, `panic`, `chr` and the float intrinsics exist; errors are handled with `??` only (the interpreter still has no `match` on results), and lambdas cannot capture the variables of the enclosing function.
