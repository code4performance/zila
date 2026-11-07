## REMOVED Requirements

### Requirement: A table
**Reason**: Its content beyond the formatting pieces (covered by their own requirements) was agreement between the native compiler and the interpreter, which no longer exists.
**Migration**: `examples/report.br` keeps its native test with the expected table.

### Requirement: Typed formatting is native
**Reason**: It existed to allow the interpreter to lack `format.fmt`; with a single implementation every `format` feature works everywhere, including under `--run`.
**Migration**: None.
