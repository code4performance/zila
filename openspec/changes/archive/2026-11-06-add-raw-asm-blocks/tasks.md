## 1. Lexer and parser

- [x] 1.1 Record the current boot image and the assembly of `examples/ffi.br` (reference for "same code")
- [x] 1.2 Raw line mode in the lexer after `asm`: comments, `end`, quoted lines, operands and clobbers left to the normal tokenizer, other lines as one text token
- [x] 1.3 Accept clobber names without quotes in the parser
- [x] 1.4 Parser and lexer tests for every scenario of the spec (keywords as mnemonics, labels, `;` comments, quoted data, `end:` label, unterminated block, old quoted form, interpolation error, operands vs `in al, dx`, clobbers both ways) and update the old `error_asm_bad_line` test

## 2. Conversion

- [x] 2.1 Convert `os/boot.br`, `os/kernel.br` and `examples/ffi.br`, and show the image and the assembly are identical to the reference
- [x] 2.2 Convert the asm blocks in `tests/*.rs`, `brasa-ffi.md`, `docs/syntax.html`, `docs/examples.html` and `docs/os.html`

## 3. Editor and documentation

- [x] 3.1 Update the TextMate grammar so raw assembly lines are highlighted
- [x] 3.2 Describe raw lines (and the still accepted quoted lines) in `docs/syntax.html`, `brasa-ffi.md` and the README
- [x] 3.3 Run the full test suite and `openspec validate add-raw-asm-blocks`
