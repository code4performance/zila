## 1. Server

- [x] 1.1 JSON reader and writer in `src/lsp.rs`, with escapes and surrogate pairs
- [x] 1.2 Message framing, `initialize`, `shutdown`, `exit`, unknown requests
- [x] 1.3 Documents: open, change, close; diagnostics from the parser with UTF-16 columns
- [x] 1.4 Formatting through `fmt::format_source`
- [x] 1.5 `--lsp` in `src/cli.rs` (`FLAGS`, `Mode::Lsp`) and `src/main.rs`

## 2. Tests and documentation

- [x] 2.1 `tests/lsp.rs`: JSON round trip, initialize, good and bad files, editing, formatting, shutdown
- [x] 2.2 `docs/getting-started.html` documents `--lsp` (checked by `tests/docs.rs`)
- [ ] 2.3 Follow-ups: VS Code extension client that starts `brasa-parse --lsp`, type errors, hover, go to definition, completion
