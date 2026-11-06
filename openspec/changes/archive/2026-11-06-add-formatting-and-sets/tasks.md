## 1. Format module

- [x] 1.1 Create `std/format.br` with `chars`, `pad_left`, `pad_right`, `center` (with and without a fill character), counting UTF-8 characters; register it in `std_source`
- [x] 1.2 Add `int`, `hex`, `bin`, `float` (over `math.fixed`) and `thousands`, including the largest and smallest 64-bit integers and negative halves
- [x] 1.3 Add `text(template, args)`: scanning, `%%`, automatic and explicit indexes, layout parsing, and the error messages of the spec

## 2. Set module

- [x] 2.1 Create `std/set.br` with the generic `Set<T>` record over `Map<T, bool>`, `empty`, `from_list`, `add`, `has`, `remove`, `clear`, `len`, `to_list`; register it in `std_source`
- [x] 2.2 Add `union`, `intersect`, `difference` and `is_subset`
- [x] 2.3 Check that element types other than `text` and integers fail with the map key error naming the type (the generic template check accepts `Map<T, bool>` and the check runs at instantiation)

## 3. Interpreter

- [x] 3.1 Add `format` and `set` to the modules the interpreter loads and close any gaps it has for them

## 4. Tests and examples

- [x] 4.1 `tests/format.rs`: every scenario of the format spec, natively, plus the same programs in the interpreter
- [x] 4.2 `tests/sets.rs`: every scenario of the set spec, including 100,000 elements, no leaks (a loop that creates and drops sets), and the unsupported element error
- [x] 4.3 `examples/report.br` and `examples/sets.br`, run by the examples tests natively and with the interpreter and compared

## 5. Documentation

- [x] 5.1 Add the `format` and `set` sections to `docs/stdlib.html` and update the README standard-library rows and roadmap row
- [x] 5.2 Run the full test suite and `openspec validate add-formatting-and-sets`
