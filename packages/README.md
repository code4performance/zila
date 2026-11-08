# Official packages

The libraries that are not part of the core of the standard library, as packages: `json`, `crypto`,
`tls`, `ui`, `web`, `ai`, `mcp`, `rag`, `db`, `postgres` and `sqlserver`. Each directory is a package
(`<name>.br` and a `brasa.toml`) and this directory is a project, so a program next to it can use
them by path: `use json` finds `packages/json` before the copy built into the compiler (and the
compiler warns that the package shadows the standard library's).

For now the compiler's own copies in `std/` are the source of truth and these directories mirror them;
`tests/packages.rs` fails when they differ. To refresh them after changing a library:

    cp std/<name>.br packages/<name>/<name>.br

Using one of these libraries as part of the standard library still works and prints a warning that it is
moving into a package of its own. Removing them from the compiler is a separate, later change.
