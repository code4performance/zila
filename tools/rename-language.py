#!/usr/bin/env python3
"""Renames the language in the whole repository in one repeatable, reviewable step.

    python tools/rename-language.py Zila --display "Zila Lang"             # dry run: changes nothing
    python tools/rename-language.py Zila --display "Zila Lang" --apply --yes

The new name is the only required input. Everything else is derived from it (see `derive`):

    old form     meaning                          new form (for `Zila`)
    brasa        identifiers, command, crate      zila
    BRASA        environment variables            ZILA
    Brasa        a word in prose                  the display name ("Zila Lang")
    Brasa<Word>  inside an identifier             Zila

The text of the old name's story (the places that explain what it meant) is never rewritten; the tool
lists those lines for a person to write again. Nothing is committed or pushed. The standard library of
Python is all it needs. See openspec/changes/rename-language for the design.
"""

import argparse
import json
import os
import re
import subprocess
import sys

OLD = {"lower": "brasa", "camel": "Brasa", "upper": "BRASA"}
OLD_REPO = "brasa-parser"

# Paths the tool never touches (prefixes of tracked paths, and single files).
EXCLUDE_PREFIXES = ["openspec/changes/archive/", "openspec/changes/rename-language/", ".git/", "target/"]
EXCLUDE_FILES = ["Cargo.lock", "tools/rename-language.py", "tools/rename-allow.txt", "tests/rename.rs"]

# A line that contains one of these tells the story of the old name: it is left alone and listed.
STORY_MARKERS = ["**ember**", "*ember*", "Brasil", "mandar brasa", "Portuguese word", "\U0001F525", "Why Brasa", "Vamos mandar"]

CHECKLIST = [
    "the crate name is free on crates.io (or the crate is not published)",
    "the GitHub organization and repository name are free",
    "the extension publisher id is free on the VS Code Marketplace and on Open VSX",
    "a domain for the site is available",
    "no existing language, tool or command has the same name",
    "the command (name + '-parse') is short and easy to type",
    "the source file extension (.br) still fits, or --ext gives the new one",
]


class Plan:
    def __init__(self, name, display=None, repo=None, publisher=None, ext=None):
        self.name = name
        self.camel = name[0].upper() + name[1:]
        self.lower = name.lower()
        self.upper = name.upper()
        self.display = display or self.camel
        self.repo = repo or self.lower
        self.publisher = publisher or self.lower
        self.ext = ext

    def forms(self):
        return {
            "display": self.display,
            "identifier": self.lower,
            "environment prefix": self.upper + "_",
            "command": self.lower + "-parse",
            "crate and library": self.lower,
            "assembly symbol prefix": self.lower + "_",
            "extension, language and setting ids": self.lower,
            "window class": self.camel + "Window",
            "repository slug": self.repo,
        }


def validate(name):
    if not re.fullmatch(r"[A-Za-z][A-Za-z0-9]*", name or ""):
        raise SystemExit("error: a name must start with a letter and use only letters and digits (got %r)" % name)


def derive(name, **kw):
    validate(name)
    return Plan(name, **kw)


def is_story(line):
    return any(m in line for m in STORY_MARKERS)


def rewrite_line(line, plan):
    line = line.replace(OLD_REPO, plan.repo)
    line = line.replace(OLD["upper"], plan.upper)
    line = line.replace(OLD["lower"], plan.lower)
    line = re.sub(r"(?<![A-Za-z0-9_])" + OLD["camel"] + r"(?![A-Za-z0-9_])", plan.display, line)
    line = line.replace(OLD["camel"], plan.camel)
    return line


def rewrite_path(path, plan):
    new = path.replace(OLD_REPO, plan.repo).replace(OLD["lower"], plan.lower).replace(OLD["camel"], plan.camel).replace(OLD["upper"], plan.upper)
    if plan.ext:
        new = re.sub(r"\.br$", "." + plan.ext, new)
    return new


def excluded(path):
    p = path.replace("\\", "/")
    return any(p.startswith(x) for x in EXCLUDE_PREFIXES) or p in EXCLUDE_FILES


def git(*args, root="."):
    r = subprocess.run(["git"] + list(args), cwd=root, capture_output=True)
    if r.returncode != 0:
        raise SystemExit("error: git %s failed: %s" % (" ".join(args), r.stderr.decode("utf-8", "replace")))
    return r.stdout.decode("utf-8", "replace")


def tracked(root):
    out = git("ls-files", "-z", root=root)
    return [p for p in out.split("\0") if p]


def read_text(path):
    if not os.path.isfile(path):
        return None  # tracked but deleted in the working tree
    with open(path, "rb") as f:
        data = f.read()
    if b"\0" in data:
        return None
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError:
        return None


def docx_occurrences(path):
    """The occurrences of the old name inside a Word document (a zip of XML), or 0."""
    import zipfile
    try:
        with zipfile.ZipFile(path) as z:
            return sum(len(re.findall("(?i)brasa", z.read(n).decode("utf-8", "replace"))) for n in z.namelist() if n.endswith((".xml", ".rels")))
    except (zipfile.BadZipFile, OSError):
        return 0


def has_old(s):
    return re.search("brasa", s, re.IGNORECASE) is not None


MIXED = re.compile(r"(?i)brasa")


def mixed_forms(text):
    return sorted({m.group(0) for m in MIXED.finditer(text) if m.group(0) not in OLD.values()})


def plan_changes(root, plan):
    """Returns (moves, edits, stories, mixed): the moves as (old, new), the edits as
    {path: (count, [(line_no, old, new)])}, the story lines as (path, line_no, text)."""
    moves, edits, stories, mixed = [], {}, [], []
    documents = []
    for path in tracked(root):
        if excluded(path):
            continue
        new = rewrite_path(path, plan)
        if new != path and os.path.exists(os.path.join(root, path)):
            moves.append((path, new))
        text = read_text(os.path.join(root, path))
        if text is None:
            if path.lower().endswith(".docx"):
                n = docx_occurrences(os.path.join(root, path))
                if n:
                    documents.append((new, n))
            continue
        changed = []
        for i, line in enumerate(text.splitlines(keepends=True), 1):
            if not has_old(line):
                continue
            if is_story(line):
                stories.append((path, i, line.rstrip("\r\n")))
                continue
            new_line = rewrite_line(line, plan)
            if new_line != line:
                changed.append((i, line.rstrip("\r\n"), new_line.rstrip("\r\n")))
            for m in mixed_forms(line):
                mixed.append((path, i, m))
        if changed:
            edits[path] = (len(changed), changed)
    return moves, edits, stories, mixed, documents


def apply_changes(root, plan, moves, edits):
    for old, new in moves:
        os.makedirs(os.path.dirname(os.path.join(root, new)) or ".", exist_ok=True)
        git("mv", old, new, root=root)
    moved = dict(moves)
    for path in edits:
        target = os.path.join(root, moved.get(path, path))
        with open(target, "rb") as f:
            data = f.read().decode("utf-8")
        out = []
        for line in data.splitlines(keepends=True):
            out.append(line if (not has_old(line) or is_story(line)) else rewrite_line(line, plan))
        with open(target, "wb") as f:
            f.write("".join(out).encode("utf-8"))


def print_rules():
    print(json.dumps({"old": OLD, "old_repo": OLD_REPO, "exclude_prefixes": EXCLUDE_PREFIXES, "exclude_files": EXCLUDE_FILES, "story_markers": STORY_MARKERS}, indent=2))


def self_test():
    p = derive("Zorbo")
    assert p.forms()["command"] == "zorbo-parse" and p.upper == "ZORBO" and p.lower == "zorbo" and p.camel == "Zorbo"
    assert rewrite_line("BRASA_UI_SCRIPT brasa-parse BrasaWindow Brasa is", p) == "ZORBO_UI_SCRIPT zorbo-parse ZorboWindow Zorbo is"
    q = derive("Zila", display="Zila Lang")
    assert rewrite_line("the Brasa compiler, Brasa's goal, BrasaWindow", q) == "the Zila Lang compiler, Zila Lang's goal, ZilaWindow"
    assert rewrite_line("code4performance/brasa-parser", q).endswith("/zila")
    assert rewrite_path("brasa-ffi.md", q) == "zila-ffi.md" and rewrite_path("docs/Brasa.docx", q) == "docs/Zila.docx"
    assert excluded("openspec/changes/archive/x/y.md") and not excluded("openspec/specs/a/spec.md") and excluded("Cargo.lock")
    assert is_story("Brasa is the Portuguese word for **ember**") and not is_story('<span class="ember-dot">') and not is_story("brasa-parse --fmt")
    assert mixed_forms("bRASA and brasa and Brasa") == ["bRASA"]
    for bad in ["9lives", "two words", "", "a-b"]:
        try:
            derive(bad)
        except SystemExit as e:
            assert "must start with a letter" in str(e)
        else:
            raise AssertionError("accepted " + repr(bad))
    # a second pass changes nothing
    once = rewrite_line("brasa BRASA Brasa BrasaWindow", q)
    assert rewrite_line(once, q) == once
    print("self-test ok")


def main():
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("name", nargs="?", help="the new name (letters and digits, starting with a letter)")
    ap.add_argument("--display", help="how the name is written in prose (default: the name with a capital)")
    ap.add_argument("--repo", help="repository slug (default: the lower-case name)")
    ap.add_argument("--publisher", help="extension publisher id (default: the lower-case name)")
    ap.add_argument("--ext", help="a new source file extension, without the dot (default: unchanged)")
    ap.add_argument("--apply", action="store_true", help="change the tree (default: a dry run)")
    ap.add_argument("--yes", action="store_true", help="confirm after the checklist (needed with --apply)")
    ap.add_argument("--force", action="store_true", help="apply on a tree with uncommitted changes")
    ap.add_argument("--root", default=".", help="the repository (default: the current folder)")
    ap.add_argument("--print-rules", action="store_true")
    ap.add_argument("--print-checklist", action="store_true")
    ap.add_argument("--self-test", action="store_true")
    a = ap.parse_args()
    if a.self_test:
        return self_test()
    if a.print_rules:
        return print_rules()
    if a.print_checklist:
        print("\n".join("- " + c for c in CHECKLIST))
        return
    if not a.name:
        ap.error("the new name is required")
    plan = derive(a.name, display=a.display, repo=a.repo, publisher=a.publisher, ext=a.ext)
    print("The name: %s  (display: %s)" % (plan.name, plan.display))
    for k, v in plan.forms().items():
        print("  %-38s %s" % (k, v))
    moves, edits, stories, mixed, documents = plan_changes(a.root, plan)
    print("\nMoves (%d):" % len(moves))
    for o, n in moves:
        print("  git mv %s %s" % (o, n))
    total = sum(c for c, _ in edits.values())
    print("\nReplacements: %d lines in %d files" % (total, len(edits)))
    for path in sorted(edits):
        count, lines = edits[path]
        print("  %s (%d)" % (path, count))
        for i, old, new in lines[:3]:
            print("      %d: %s" % (i, old.strip()[:110]))
            print("      -> %s" % new.strip()[:110])
        if count > 3:
            print("      ... %d more" % (count - 3))
    if mixed:
        print("\nMixed-case forms the tool will not replace (%d):" % len(mixed))
        for p, i, m in mixed:
            print("  %s:%d: %s" % (p, i, m))
    print("\nWritten by a person, not by the tool (the story of the name, %d lines):" % len(stories))
    for p, i, t in stories:
        print("  %s:%d: %s" % (p, i, t.strip()[:100]))
    if documents:
        print("\nWord documents with the old name inside (edit them, or delete them):")
        for d, n in documents:
            print("  %s: %d occurrences" % (d, n))
    if not a.apply:
        print("\nDry run: nothing was changed.")
        return
    if not moves and not edits:
        print("\nNothing to do.")
        return
    print("\nBefore applying, check:")
    for c in CHECKLIST:
        print("  [ ] " + c)
    if not a.yes:
        raise SystemExit("\nThe checklist is above. Run again with --yes to apply the name '%s'." % plan.name)
    if not a.force and git("status", "--porcelain", root=a.root).strip():
        raise SystemExit("error: the working tree has uncommitted changes; commit them, or use --force")
    apply_changes(a.root, plan, moves, edits)
    print("\nApplied: %d moves, %d lines. Nothing was committed. Now: cargo build, the tests, and rewrite the lines listed above." % (len(moves), total))


if __name__ == "__main__":
    main()
