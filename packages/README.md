<img src="../docs/assets/zila.svg" alt="Zila Lang" width="48" height="48">

# Official packages

*A small, readable language that compiles straight to machine code.*

The libraries that are not part of the core of the standard library, as packages: `json`, `crypto`,
`tls`, `ui`, `web`, `ai`, `mcp`, `rag`, `db`, `postgres` and `sqlserver`. Each directory is a package
(`<name>.zila` and a `zila.toml`), and this directory is a project, so a program next to it can use
them by path: `use json` finds `packages/json` before the copy built into the compiler, and the
compiler warns that the package shadows the standard library's.

These files are the only copy: the compiler embeds them when it is built, so a program with no
`zila.toml` still says `use json` and gets them. `ui` and `tls` also have a Linux layer in
`<name>/linux/<name>.zila`, which the compiler uses for `--target linux` (it is not part of the package
for Windows).
