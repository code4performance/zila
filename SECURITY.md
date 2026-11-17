<img src="docs/assets/zila.svg" alt="Zila Lang" width="48" height="48">

# Security Policy

*A small, readable language that compiles straight to machine code.*

## Reporting a vulnerability

If you believe you have found a security vulnerability in Zila Lang — for example, compiler-generated code that is unsafe in a way the language does not document, or a flaw in a standard-library package that handles untrusted input — please report it **privately**, not in a public issue or pull request.

**Preferred: GitHub Security Advisories.** Open a private report via the "Report a vulnerability" button on the repository's Security tab (`code4performance/zila` → Security → Advisories → Report a vulnerability). This reaches the maintainers directly and keeps the report private until a fix is ready.

**If that is not available to you**, follow the private-contact process in [CODE_OF_CONDUCT.md#reporting-a-concern](CODE_OF_CONDUCT.md#reporting-a-concern): use the contact details on the GitHub profile of the repository owner (`code4performance`), or open an issue that says only that you need a private contact, without describing the problem.

Please include, as far as you can:

- The smallest program or input that shows the problem.
- What you expected and what actually happened.
- The potential impact, as you understand it.

## What happens next

A maintainer will acknowledge your report and work with you on a fix before any public disclosure. Please give us a reasonable time to respond before disclosing publicly.

## Scope

This policy covers the Zila Lang compiler, standard library and packages in this repository. It does not cover vulnerabilities in programs written *in* Zila Lang by other people — report those to the authors of that program.
