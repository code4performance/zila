## Purpose

Windows and controls for programs, with one set of functions and one behavior on Windows and on Linux, so the same source builds and runs on both.

## ADDED Requirements

### Requirement: One library on both targets
`use ui` SHALL compile with `--target windows` and with `--target linux` from the same source and SHALL offer the same names, parameter lists, defaults and result types on both: `Window(title, width, height)`, `TextBox(value)`, `Button(label)`, `ListView()`, `Label(value)`, `vstack(...)`, `hstack(...)`, the properties `text`, `count` and `selected`, the methods `set_text`, `add`, `remove`, `clear`, `onClick`, `onChange`, `onSelect`, `layout`, `show`, `close`, and the functions `run` and `alert`. A program SHALL NOT need to know which target it runs on, and the library SHALL expose nothing that exists on one target only.

#### Scenario: The example builds for both
- **WHEN** `examples/ui.br` is built with `--target windows` and with `--target linux`
- **THEN** both builds succeed with no change to the source

#### Scenario: No platform names in the public surface
- **WHEN** the public record and function declarations of the Windows and Linux layers are compared
- **THEN** they are identical

### Requirement: Controls
A `TextBox` SHALL hold editable single-line text; typing, Backspace, Delete, Left, Right, Home and End SHALL edit it and `onChange` SHALL run after each change. A `Button` SHALL run `onClick` when it is clicked, and when it is the first button of the window and Enter is pressed in a text box. A `ListView` SHALL show its items, scroll when they do not fit, let one item be selected by click or by Up and Down, and run `onSelect` when the selection changes. A `Label` SHALL show text that cannot be edited. `count` SHALL be the number of items of a list and `selected` the zero-based index of the selected item or -1. Setting the `text` of a control from the program SHALL run its `onChange` handlers exactly once when the text changed and not at all when it did not. After a window is closed, its controls SHALL keep answering `text`, `count` and `selected` with the values they held when it closed.

#### Scenario: Add to a list
- **WHEN** the user types `Ana` in the text box and clicks the Add button of `examples/ui.br`
- **THEN** the list shows `Ana` and the text box is empty

#### Scenario: Setting the same text
- **WHEN** a program sets a text box to the text it already holds
- **THEN** no `onChange` handler runs

#### Scenario: After closing
- **WHEN** a program reads `list.count` after its window was closed
- **THEN** it gets the number of items the list had when the window closed, on both targets

#### Scenario: Selection
- **WHEN** the user clicks the second of three items
- **THEN** `selected` is 1 and `onSelect` has run once

### Requirement: Layout
`layout(root)` SHALL arrange the controls of `vstack` and `hstack` in a window the same way on both targets: padding and gaps in logical pixels scaled by the display's resolution, text boxes, buttons and labels at their preferred height, lists taking the remaining space, and the arrangement recomputed when the window is resized.

#### Scenario: Resizing
- **WHEN** the window is made taller
- **THEN** the list grows and the text box and the button keep their height

### Requirement: Showing windows and the event loop
`show()` SHALL make a window visible. `run()` SHALL handle input and redraw until the last window is closed, then return; a handler that calls `close()` SHALL close its window. Closing a window with the window manager's close button SHALL behave as `close()`. Handlers SHALL run one at a time on the thread that called `run()`.

#### Scenario: Last window closed
- **WHEN** the only window is closed
- **THEN** `run()` returns and the program continues after it

### Requirement: Alert
`alert(message, title)` SHALL show a box with the message, the title (default `Brasa`) and an OK button, and SHALL return after the user dismisses it with the button, Enter or (on Linux) Escape. While `BRASA_UI_SCRIPT` is set, `alert` SHALL return at once without showing anything, on both targets.

#### Scenario: Dismissing
- **WHEN** `alert("saved")` is called and the user presses Enter
- **THEN** the box closes and the call returns

### Requirement: Failure to open a display
When no window system can be reached (on Linux: no usable local X display; on Windows: the system refuses to create a window), the first call that needs it SHALL print `error: ui: cannot open a display: <reason>` and stop the program with status 1. The reason SHALL say what is wrong: `DISPLAY is not set`, `cannot connect to <socket>`, `only local displays are supported`, or the server's refusal.

#### Scenario: No display
- **WHEN** a Linux program that opens a window runs with `DISPLAY` unset and no X socket
- **THEN** it exits with status 1 and prints `error: ui: cannot open a display: ...`

### Requirement: Synthetic input for tests
A program built for either target SHALL be drivable without a person: when the environment variable `BRASA_UI_SCRIPT` names a file, the library SHALL read it as a list of input steps (`click <index>`, `type <text>`, `key <name>`, `select <index>`, `close`), one per line (`key` takes `backspace`, `enter`, `up` or `down`; typed text is appended at the end of the first text box), and apply them in order to the first window as if the user had done them, then close the window. A step that has no target (no text box, no list, no such button) SHALL be ignored. Programs and their output SHALL otherwise be unaffected.

#### Scenario: Scripted run
- **WHEN** `examples/ui.br` runs with a script that types `Ana`, clicks the Add button and closes the window
- **THEN** the program's handlers run in that order and `run()` returns
