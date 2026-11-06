## 1. Candidate table

- [x] 1.1 Add the inlinability check (parameter and result types, body shape, size limit, no self call) and the `inline_bodies` table, built when signatures are collected and only when `opt` is on
- [x] 1.2 Test the check on a set of functions (eligible, and each kind of ineligible one)

## 2. Inlining at the call site

- [x] 2.1 In `call_sig`, compile the body in place: isolated scope with the parameters, callee module, result checked and normalized like a `return`, nesting depth limit
- [x] 2.2 Alias literal and plain-local arguments instead of copying them into temporaries
- [x] 2.3 Make sure nothing is inlined with `--no-opt`, through function values, or for functions that are ineligible

## 3. Tests

- [x] 3.1 Differential tests: evaluation order and single evaluation, unused parameters, name collisions with locals and with module globals, a library function inlined into main, narrow integer and `f32` helpers, run-time errors inside a helper, nested helpers
- [x] 3.2 Assembly tests: the call is gone for the eligible function and present for each ineligible kind and with `--no-opt`
- [x] 3.3 Random programs with helper functions and colliding names agree with and without optimization
- [x] 3.4 Add the helper-calls benchmark with the 25% assertion and record the numbers of all benchmarks in the design

## 4. Documentation

- [x] 4.1 Update `docs/native.html` (optimizer section) and the README status and roadmap rows
- [x] 4.2 Run the full test suite and `openspec validate add-inlining`
