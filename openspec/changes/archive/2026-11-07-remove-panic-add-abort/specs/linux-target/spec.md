## MODIFIED Requirements

### Requirement: The same programs give the same output
A program that uses only the language and the libraries available on Linux SHALL print the same text on Linux as on Windows, including numbers, floats, text with multibyte characters, collections, errors and `format`. A program whose `main` returns an error SHALL print `error: <message>` and exit with status 1; an `abort` SHALL print `abort: <file>.br:<line>: <message>` to standard error and exit with status 101; normal completion SHALL exit with status 0. A run-time failure such as division by zero SHALL be an abort.

#### Scenario: Examples agree
- **WHEN** the example programs that do not use the Windows API are built for both targets and run
- **THEN** their output is identical

#### Scenario: Exit codes
- **WHEN** a program's `main` returns `error("boom")`, and another calls `abort("stop")`
- **THEN** the first prints `error: boom` and exits with 1, and the second prints `abort: <file>.br:<line>: stop` and exits with 101

### Requirement: Memory
Objects SHALL be reference counted and freed as on Windows, `live_objects()` SHALL count them, and memory SHALL be given back: a loop that builds and drops large texts and lists SHALL NOT grow without bound. Running out of memory SHALL abort the program with `abort: out of memory`.

#### Scenario: Out of memory
- **WHEN** a program keeps allocating small objects that stay alive until the heap is full
- **THEN** it prints `abort: out of memory` and exits with status 101

#### Scenario: No leaks and big values
- **WHEN** a program builds and drops 160,000-byte texts and lists of 2,000 elements hundreds of times, and a list of 300,000 integers is built and summed, and `live_objects()` is compared before and after
- **THEN** the count has not grown and the program finishes in reasonable memory
