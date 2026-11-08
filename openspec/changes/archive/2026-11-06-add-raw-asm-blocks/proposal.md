## Why

Every assembly line in an `asm` block has to be written as a string (`"mov ax, 1"`). The quotes add noise, force escaping (`db \"text\", 0`) and make the code look unlike assembly; `os/boot.br` has 137 quoted lines. The lexer reads Zila Lang tokens, which is why assembly cannot simply be written bare today (`in`, `out`, `or` and `not` are Zila Lang words, `;` and `label:` are not Zila Lang tokens). The block form is unambiguous, so the lexer can read its lines as they are.

## What Changes

- Inside an `asm ... end` block, each line is assembly written without quotes: the lexer takes the rest of the line as the instruction text, up to a `//` comment. Labels (`start:`), memory operands (`[rax]`), `;` comments, numbers and mnemonics that are Zila Lang words (`in al, dx`, `out dx, al`, `or al, 2`) work as written; text in assembly quotes (`db "Kernel loaded", 13, 10, 0`) needs no escaping.
- A line that starts with `in`, `out` or `inout`, a name and `=` is still an operand (`in rdx = port`); a line starting with `clobber` is still a clobber list, and its entries may be written with or without quotes (`clobber cc, memory`). The block ends at a line that is just `end`.
- Lines written as strings are still accepted, so existing code keeps working.
- Everything in the repository that uses `asm` is converted to the new form: `os/boot.br`, `os/kernel.br`, `examples/ffi.br`, the tests, `zila-ffi.md`, the docs site, and the VS Code grammar highlights the raw lines.

## Capabilities

### New Capabilities

- `inline-assembly`: how `asm` blocks are written and read (raw lines, operands, clobbers, comments, the end of the block, the old string form).

### Modified Capabilities

None.

## Impact

- `src/lexer.rs` (raw line mode after `asm`), `src/parser.rs` (clobber names without quotes), `tests/parse.rs` and a new lexer-level test for the cases above.
- `os/boot.br`, `os/kernel.br`, `examples/ffi.br`, `tests/optimizer.rs`, `zila-ffi.md`, `docs/syntax.html`, `docs/examples.html`, `docs/os.html`, `editors/vscode/syntaxes/zila.tmLanguage.json`.
- The produced machine code of every converted program stays byte-for-byte the same (the boot image is compared before and after).
