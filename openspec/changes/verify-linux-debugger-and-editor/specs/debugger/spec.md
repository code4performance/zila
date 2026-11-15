## MODIFIED Requirements

### Requirement: Platforms
The adapter SHALL work for programs built for the host operating system, on Windows and on Linux. A program whose target cannot be debugged on the host SHALL fail the launch with a message naming the reason.

#### Scenario: Wrong target
- **WHEN** a Linux program is launched on Windows, or a Windows program on Linux
- **THEN** the launch fails with a message that says so

#### Scenario: Linux session
- **WHEN** the scripted session of a sample program (breakpoint, steps, stack, variables) is run on Linux
- **THEN** it passes as it does on Windows
