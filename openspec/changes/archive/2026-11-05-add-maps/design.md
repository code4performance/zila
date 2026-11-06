## Context

The parser already accepts `{ k: v }` literals (`ExprKind::Map`) and `Map<K, V>` types (`Type::Named`), and the native backend answers "not supported yet". Lists are a reference-counted heap object (`[len][cap][data][elements-are-heap]`) with `rt_list_*` procedures written in assembly inside `codegen_x64.rs`, and element ownership is tracked through `is_rc`, `retain_if` and per-list drop procedures. Maps should follow that same shape so memory rules stay uniform.

## Goals / Non-Goals

**Goals**
- A map that is simple to use, deterministic to iterate, and leak-free under the existing live-object test.
- Reuse the list machinery for ownership, iteration code and method dispatch wherever possible.

**Non-Goals**
- Sets, sorted maps, record/enum/float keys, custom hash functions.
- Making the interpreter fast; it only needs to agree with the native behavior.

## Decisions

1. **Type name `Map<K, V>`.** It is already what the docs and grammar use. `Ty::Map(Box<Ty>, Box<Ty>)` is added next to `Ty::List`; it is nullable and reference counted like a list.
2. **Insertion-ordered open hash table.** The object holds `entries` (a dense array of `[hash][key][value]` in insertion order) and `index` (an open-addressing table of entry numbers, power-of-two size, grown at 70% load). Iteration walks `entries`, which gives deterministic order and keeps `for` the same shape as the list loop.
3. **Removal marks a tombstone** in `entries` and in `index`; the table is rebuilt (compacted) when tombstones exceed half of the entries. Iteration skips tombstones.
4. **Keys: `text` and integers.** Text hashes its bytes (FNV-1a) and compares by length and bytes; integers hash their 64-bit value and compare by value. Anything else is a compile error naming the key type.
5. **Ownership.** A map holding heap keys or values (flags stored in the header, as for lists) retains what it stores and releases it on drop, on `remove`, on overwrite and on `clear`. Reads retain the value they hand out, as callers of `rt_list_get` do.
6. **Missing key.** `m[key]` on a missing key panics through `rt_panic` with "map key not found". `get(key, default)` and `has(key)` are the safe forms. No optional return type, since optionals work only for reference types today.
7. **Literals and inference.** Types come from the expected type (`let m: Map<text, int> = {}`) or the first pair, exactly as list literals do. The literal is built with `rt_map_new` and `rt_map_set` calls.
8. **Interpreter.** A map value backed by an insertion-ordered vector plus a `HashMap` from key to position, with the same panic message.
9. **Bare-metal.** The OS target rejects maps with a clear message if the runtime they need is unavailable there.

## Risks / Trade-offs

- **The assembly hash table is the riskiest part.** Mitigation: build in small steps (new/set/get, then growth, then remove), each with a program test, plus the 5000-call live-object leak test used for `defer`.
- **Compaction while iterating** could shift entries. Mitigation: compaction only happens on `set` and `remove`; the loop re-reads the length each step and bounds-checks, so a mistake is unspecified but memory-safe.
- **Text hashing cost** on every lookup. Acceptable now; the optimizer item on the roadmap can cache hashes later.
- **No missing-key optional** makes `m[key]` panic-prone. Accepted: `has` and `get` with a default cover the safe cases, and an optional form can come once optionals support non-reference types.
