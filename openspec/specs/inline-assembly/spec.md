## Purpose
How `asm` blocks are written and read: raw assembly lines, operands, clobbers, comments, the end of the block and the older string form.

## Requirements

### Requirement: Assembly lines are written without quotes
Inside an `asm` block that starts on its own line, each line that is not an operand, a clobber list, a comment or the end of the block SHALL be taken as one line of assembly exactly as written (leading and trailing spaces removed, a trailing `//` comment removed unless it is inside assembly quotes). Words that are Zila Lang keywords (`in`, `out`, `or`, `and`, `not`), labels, `;` comments, memory operands, numbers and quoted assembly text SHALL be passed to the assembler unchanged and without escaping.

#### Scenario: Instructions that look like Zila Lang words
- **WHEN** a block contains the lines `in al, dx`, `out dx, al` and `or al, 2`
- **THEN** each is one assembly line with the same text

#### Scenario: Labels, comments and quoted data
- **WHEN** a block contains `start:`, `mov ax, [bx] ; load`, `msg: db "Hello", 13, 10, 0` and `// a Zila Lang comment`
- **THEN** the first three are assembly lines (the `;` comment is left to the assembler, the data text keeps its quotes) and the last is ignored

#### Scenario: Same machine code
- **WHEN** a program using quoted lines is rewritten with raw lines
- **THEN** the compiled output is identical

### Requirement: Operands and clobbers keep their forms
A line starting with `in`, `out` or `inout`, then a name, then `=` SHALL be an operand whose value is a Zila Lang expression, and a line starting with `clobber` SHALL be a list of register or state names, each written with or without quotes, separated by commas. `in al, dx` is not an operand.

#### Scenario: Operand and instruction with the same first word
- **WHEN** a block contains `in al, dx` and `in rdx = port`
- **THEN** the first is an assembly line and the second binds `rdx` to `port`

#### Scenario: Clobbers
- **WHEN** a block contains `clobber cc, memory` or `clobber "cc", "memory"`
- **THEN** both give the clobber list `cc`, `memory`

### Requirement: The end of the block
The block SHALL end at the first line that consists only of `end` (optionally followed by a comment); a line that merely starts with `end` (such as the label `end:`) SHALL be assembly. A block without such a line SHALL be reported as unterminated.

#### Scenario: A label named end
- **WHEN** a block contains the line `end:` before its final `end`
- **THEN** `end:` is an assembly line

#### Scenario: Missing end
- **WHEN** the source stops inside an `asm` block
- **THEN** the compiler reports that the `asm` block is unterminated

### Requirement: Quoted lines are still accepted
A line in an `asm` block that starts with a double quote SHALL still be read as a Zila Lang string literal (without interpolation), so programs written before raw lines keep working.

#### Scenario: Old form
- **WHEN** a block is written with `"cli"` and `"hlt"`
- **THEN** it compiles to the same code as with bare `cli` and `hlt`

#### Scenario: Interpolation is rejected
- **WHEN** a quoted line contains `{x}`
- **THEN** the compiler reports that an assembly line must be plain text

### Requirement: The repository uses the raw form
The boot sector, the kernel, the examples, the tests and the documentation SHALL write assembly without quotes, and the editor grammar SHALL highlight raw assembly lines in `asm` blocks.

#### Scenario: Boot image
- **WHEN** `os/boot.zila` and `os/kernel.zila` are built after the conversion
- **THEN** the image is byte-for-byte equal to the one built before
