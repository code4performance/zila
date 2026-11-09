## Why

Typing an accented letter at the prompt of a program that reads `fs.stdin()` on Windows made it abort: `abort: text.zila:254: text position 10 is not on a character boundary`. The console hands over what is typed in its input code page (CP850 and similar), where `ç` is the single byte `0x87`; Zila text is UTF-8, so the byte is invalid text and any operation that slices around it aborts. The runtime already switches the console's *output* to UTF-8 at start-up; the input was left out of `add-stdin-reader`, and its tests used pipes, which carry bytes as they are and never exercised a console.

## What Changes

- On Windows, `fs.stdin()` switches the console's input code page to UTF-8 (`SetConsoleCP(65001)`), as the runtime does for output, so text typed at a terminal arrives as UTF-8. When standard input is not a console the call has no effect.
- The stdin-reader spec gains the requirement and the documentation says so.

## Capabilities

### New Capabilities

None.

### Modified Capabilities
- `stdin-reader`: text typed at a Windows terminal arrives as UTF-8.

## Impact

`std/fs.zila` (one extern and one call in `stdin()`), `docs/async.html`, `openspec/specs/stdin-reader`. Linux needs nothing: a terminal there delivers the bytes of the user's locale, UTF-8 in practice. Not covered by an automated test: a console cannot be driven from the test harness (it has no pseudo-console); the piped UTF-8 case is tested and the console case was checked by hand by the maintainer.

Non-goals: converting other code pages for pipes and files (their bytes are the program's to interpret, as for any file), restoring the previous code page on exit, and a text type that tolerates invalid UTF-8.
