# Zila Lang on GitHub (Linguist)

GitHub highlights a file and fills the repository's language bar using
[Linguist](https://github.com/github-linguist/linguist). Until Linguist knows `.zila`, GitHub shows
`.zila` files as plain text and the language bar does not list Zila Lang. Nothing in this repository
can change that on its own; Linguist has to accept the language. This folder holds what its
maintainers ask for, ready to use.

## What Linguist asks for

Check the current rules in Linguist's `CONTRIBUTING.md` ("Adding a language") before opening the
pull request; they change. As of this writing:

- the language has real use: the extension appears in a large number of public repositories of
  different owners (Linguist's guideline has been about 200 repositories over the last year);
- a TextMate grammar under an accepted open-source licence: `editors/vscode/syntaxes/zila.tmLanguage.json`
  (scope `source.zila`) in this repository, licensed MIT or Apache-2.0;
- sample files that show the language: `samples/Zila/` (four programs copied from `examples/`);
- an entry in `lib/linguist/languages.yml`: `languages.yml` here.

## Steps for the pull request

1. Fork `github-linguist/linguist` and clone it.
2. Add the entry from `languages.yml` here to `lib/linguist/languages.yml`, in alphabetical order.
3. Copy `samples/Zila/*.zila` to `samples/Zila/` in Linguist.
4. Add the grammar as a submodule with Linguist's script, pointing at this repository:
   `script/add-grammar https://github.com/code4performance/zila`.
5. Generate the language id and the other derived files with `script/update-ids` and the checks in
   Linguist's contributing guide, then run its tests.
6. Open the pull request and describe where the language is used.

## Until then

`.gitattributes` at the root marks `docs/` as documentation so the language bar describes the code
(Rust) and not the HTML of the documentation site.
