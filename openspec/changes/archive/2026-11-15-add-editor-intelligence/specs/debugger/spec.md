## ADDED Requirements

### Requirement: Debug information
When built with `--debug`, `zila` SHALL write a debug-information file next to the executable that maps machine-code addresses to source file and line, and describes each function, parameter and local variable with its name, type and location. The executable itself SHALL be byte-identical with and without `--debug`.

#### Scenario: Opt in
- **WHEN** a program is built with and without `--debug`
- **THEN** the executables are byte-identical and only the first has a debug-information file

### Requirement: Debug adapter
`zila --dap` SHALL run a Debug Adapter Protocol server on standard input and output. For a launch request naming a `.zila` program it SHALL build it with `--debug`, start it under the debugger, and support line breakpoints, continue, pause, step over, step into, step out, the call stack with source positions, and stopping when the program aborts.

#### Scenario: Breakpoint
- **WHEN** a breakpoint is set on a line of an executable statement and the program reaches it
- **THEN** execution stops, the stack shows the Zila functions with file and line, and continue resumes it

#### Scenario: Line without code
- **WHEN** a breakpoint is set on a blank or comment line
- **THEN** it is moved to the next line that has code and reported as moved

### Requirement: Inspecting values
While stopped, the adapter SHALL show parameters and local variables of the selected frame with Zila Lang types and values: integers, floats, `Text`, `Char` and `Bool` as their values, records, lists, maps and enums expanded into their parts, and references as their target. A value that cannot be read SHALL be shown as unavailable, never as a wrong value. Names and field accesses typed in the debug console SHALL be evaluated.

#### Scenario: Record local
- **WHEN** stopped in a function with a local record
- **THEN** its fields are listed with their names and values

#### Scenario: Optimized-away value
- **WHEN** the optimizer removed a variable
- **THEN** it is shown as unavailable

### Requirement: Platforms
The adapter SHALL work on Windows for programs built for Windows. A program whose target cannot be debugged on the host SHALL fail the launch with a message naming the reason. Debugging on Linux is specified by a later change.

#### Scenario: Wrong target
- **WHEN** a Linux program is launched on Windows
- **THEN** the launch fails with a message that says so
