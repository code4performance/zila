## 1. Types and literals

- [x] 1.1 Add `Ty::Map`, resolve `Map<K, V>` (rejecting key types other than `text` and integers), and make it nullable, reference counted and printable in error messages; verify with compile tests for a valid type and a record key
- [x] 1.2 Compile map literals with inference from the expected type or the first pair, mismatch errors and the "needs a type annotation" error for `{}`; verify with compile-error tests and a program printing `len`

## 2. Runtime hash table

- [x] 2.1 Write `rt_map_new`, `rt_map_set` and `rt_map_get` (insertion-ordered entries plus open-addressing index, text and integer hashing); verify with a program storing and reading text and integer keys
- [x] 2.2 Add growth and rehashing; verify a program inserting 10000 keys finds every one
- [x] 2.3 Add `rt_map_has`, `rt_map_remove` (tombstones and compaction) and `rt_map_clear`; verify the 10000-key insert/remove scenario and re-adding a removed key
- [x] 2.4 Add the drop procedures and ownership rules for heap keys and values (retain on store, release on replace, remove, clear and drop); verify with `live_objects()` after 5000 calls that build, change and drop maps of texts and records

## 3. Language operations

- [x] 3.1 Compile `m[key]` reads (panic "map key not found" on a miss) and `m[key] = value` writes; verify with tests for insert, replace and a missing key
- [x] 3.2 Compile the methods `has`, `get`, `remove` and `clear` and the `len` property; verify each with a small program
- [x] 3.3 Compile `for key, value in m` and `for key in m` in insertion order; verify order, replaced values and a re-added key
- [x] 3.4 Verify maps as parameters, return values, record fields and list elements, and that assignment shares rather than copies

## 4. Interpreter

- [x] 4.1 Add map values, literals, reads, writes, methods, iteration order and the missing-key message to the interpreter; verify with `--run` on the same programs used for the native tests

## 5. Other targets

- [x] 5.1 Make the bare-metal target reject maps with a clear message if the runtime they need is unavailable there; verify with a compile-error test

## 6. Documentation and examples

- [x] 6.1 Add `examples/maps.br` (word count) and a section for it on `docs/examples.html` (generated with `zila-parse --docs`); verify the example builds and prints the documented output
- [x] 6.2 Document maps on `docs/syntax.html` (type, literal, indexing, methods, iteration order, missing key), remove them from the not-yet-supported list in `docs/native.html`, and update the status table and roadmap in `README.md`; verify `cargo test` (docs tests) passes and every code sample compiles
