## Context

The lexer (`src/lexer.rs`) turns the whole source into tokens before the parser runs, and `asm` is a keyword. The parser's `asm_block` reads `Tok::Text` lines (plain text only, no interpolation), operands (`in|out|inout name = expr`) and `clobber "x", ...`. The assembler (`src/asm_x86.rs`) strips `;` comments itself and respects quotes.

## Goals / Non-Goals

**Goals**
- Assembly written as assembly, with no quotes or escapes, with no change to the produced code.
- Old code keeps compiling.

**Non-Goals**
- Checking the assembly syntax in the lexer (the assembler does that), multi-line macros, or a new operand syntax.

## Decisions

1. **The lexer switches to line mode after `asm`.** After pushing `Kw("asm")`, if the rest of the line is blank (or a `//` comment) the lexer sets `in_asm`. Each time it passes a newline while `in_asm`, `asm_lines` looks at the next line before normal tokenizing resumes. Block form on its own line is the only form the parser accepts, so nothing else changes.
2. **Per line in `asm_lines`** (leading blanks skipped, the line read to its end):
   - empty or starting with `//`: skipped;
   - only `end` (optionally followed by `//` comment): `in_asm` ends, normal tokenizing handles `end`;
   - starts with `"`, starts with `in|out|inout <ident> =` (and not `==`), or starts with the word `clobber` followed by a blank: left to the normal tokenizer, which reads the line as before;
   - anything else: pushed as one `Tok::Text` with a single literal part, the text trimmed and stripped of a trailing `//` comment found outside single or double quotes. The newline after it is handled by the normal loop, so the parser sees `Text Newline` exactly as with a quoted line.
3. **The parser changes only for clobbers**: an entry may be a text or an identifier (`clobber cc, memory`). Operands and the `end` keyword are untouched.
4. **Compatibility.** Because a raw line cannot start with a quote (no instruction does), accepting quoted lines is unambiguous; the tests keep a quoted block next to a raw one. A bare `nop` used to be a parse error ("expected an assembly line"); it is now valid, so that test changes to check the new error cases (unterminated block).
5. **Conversion.** A script rewrites every quoted line inside an `asm` block (unescaping `\"`, `\\`, `\{`) in `.br` files, Rust test sources and the docs; the boot image and the generated assembly of `examples/ffi.br` are compared before and after to prove nothing changed. HTML examples keep their `<span>` colouring minus the quotes.
6. **Editor grammar.** The TextMate `asm-blocks` rule highlights comments, `@operands`, registers, numbers and mnemonic-looking labels in bare lines in addition to the old string rule.

## Risks / Trade-offs

- A bare line starting with `in`/`out` followed by `name =` would be read as an operand; no x86 instruction has that shape (`mov x = y` is not assembly), so no ambiguity.
- A `//` inside assembly outside quotes is never meaningful, so stripping it is safe; inside quotes it is kept (`db "http://x"`).
- Line numbers in diagnostics stay right because the lexer advances through the line with `bump`.
