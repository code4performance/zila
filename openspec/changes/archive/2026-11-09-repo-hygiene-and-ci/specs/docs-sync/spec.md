## ADDED Requirements

### Requirement: The README roadmap follows the open changes
The roadmap in `README.md` SHALL name, in backticks, exactly the changes that are open in `openspec/changes/` (not archived), and SHALL say that there are no open changes exactly when there are none. A change that is archived SHALL NOT be named there as pending.

#### Scenario: An archived change is still listed
- **WHEN** the README names in backticks a change that is not in `openspec/changes/`
- **THEN** the test run fails and names the change

#### Scenario: An open change is missing
- **WHEN** a change is open in `openspec/changes/` and the README does not name it
- **THEN** the test run fails and names the change
