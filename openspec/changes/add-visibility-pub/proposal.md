## Why

Today every top-level function, type, variable and every record field is reachable from any other package that says `use` on it. A library cannot hide its helpers (`text.index_from`, `ws` frame internals, a record's invariants), so renaming an internal name is a breaking change for callers and nothing stops a program from reaching into a library's representation. Brasa is pre-1.0 and all Brasa code lives in this repository, so this is the cheapest moment to make the public surface explicit.

## What Changes

- **BREAKING** New reserved word `pub`. Declarations are private by default: visible in every file of the same `package`, invisible to other packages. `pub` exports a declaration.
- `pub` applies to: functions (including `async` and `extern`), methods, types (records, enums, interfaces and aliases; enum variants follow the type), top-level `let` / `var`, and record fields (`pub name: text`; fields default to private).
- Using a private name from another package is a compile-time error that says the name is private, distinct from "undefined".
- A record literal may only name fields that are visible; a record type with a private field cannot be constructed with a literal outside its package (the library must offer a constructor function). Reading or writing a private field from another package is an error.
- A `pub` declaration whose signature names a private type is an error (no leaking of private types).
- Generic code and monomorphized instances keep the access rights of the package that wrote them, not of the caller.
- The whole standard library (`std/*.br`, including the `.linux` variants), `os/`, `examples/`, the tests' embedded Brasa sources and the docs are migrated to mark what is public. The formatter, the language server and the VS Code grammar learn `pub`.

## Capabilities

### New Capabilities

- `visibility`: private-by-default declarations, the `pub` modifier, and how packages, `use`, records and generics respect it.

### Modified Capabilities

None. The existing library specs (`text-library`, `list-library`, ...) describe functions that stay public; the change to each library is the added `pub`, verified by its existing tests.

## Impact

`brasa.ebnf`, `src/lexer.rs` (`KEYWORDS`), `src/ast.rs`, `src/parser.rs`, `src/codegen_x64.rs` (declare, qualified calls, member, field access, record literal, generics), `src/interp.rs` (ignores `pub`), `src/fmt.rs`, `src/lsp.rs`, `editors/vscode/syntaxes/brasa.tmLanguage.json`, every file in `std/`, `os/*.br`, `examples/*.br`, `tests/*.rs` with embedded sources, `docs/*.html` (`tests/docs.rs` checks that `syntax.html` mentions every reserved word). Does not touch `openspec/changes/add-ui-linux` or the scope of the other planned changes (see design "Depends on").
