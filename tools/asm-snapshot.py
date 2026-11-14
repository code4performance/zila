#!/usr/bin/env python3
"""Takes and compares snapshots of the assembly the compiler writes for the repository's examples.

Used to check that a change meant to be internal does not change the output (spec `static-linking`).

    python tools/asm-snapshot.py take <zila-executable> <out-dir>
    python tools/asm-snapshot.py compare <dir-a> <dir-b>      (exit 1 when they differ)
    python tools/asm-snapshot.py compare --labels <dir-a> <dir-b>
        the same, but the numbers in generated names (`ret_1103`, `sdzero_1140`) are renamed in the
        order they first appear, per prefix, before comparing: two outputs that differ only in how
        the compiler numbered its labels are equal. Use it for a change that stops the compiler from
        compiling something it throws away (which used up label numbers); the strict mode stays the
        gate for every other internal change.

A snapshot holds, for every `examples/*.zila` and `examples/packages/*.zila`, the output of `--asm`
for the default build, `--target linux` and `--no-opt` (`<file>.<variant>.asm`, and `.err` with what
the compiler printed to standard error). Run it from the repository root.
"""
import filecmp
import glob
import os
import re
import shutil
import subprocess
import sys

VARIANTS = {"default": [], "linux": ["--target", "linux"], "noopt": ["--no-opt"]}


def take(exe, out):
    exe = os.path.abspath(exe)
    if not os.path.isfile(exe) and os.path.isfile(exe + ".exe"):
        exe += ".exe"
    shutil.rmtree(out, ignore_errors=True)
    os.makedirs(out)
    files = sorted(glob.glob("examples/*.zila") + glob.glob("examples/packages/*.zila"))
    for f in files:
        base = f.replace("/", "_").replace("\\", "_")
        for name, flags in VARIANTS.items():
            r = subprocess.run([exe, f, "--asm"] + flags, capture_output=True)
            with open(os.path.join(out, f"{base}.{name}.asm"), "wb") as o:
                o.write(r.stdout)
            with open(os.path.join(out, f"{base}.{name}.err"), "wb") as o:
                o.write(r.stderr)
    print(f"{len(files)} programs, {len(files) * len(VARIANTS)} compilations written to {out}")
    return 0


NUMBERED = re.compile(r"\b([A-Za-z][A-Za-z0-9]*(?:_[A-Za-z][A-Za-z0-9]*)*)_(\d+)(?!\d)")


def canonical(text):
    """Renames the number of every generated name to its rank among the numbers of the same prefix."""
    ranks = {}

    def rename(m):
        seen = ranks.setdefault(m.group(1), {})
        return f"{m.group(1)}_{seen.setdefault(m.group(2), len(seen))}"

    return NUMBERED.sub(rename, text)


def same_file(a, b, labels):
    if not labels or not a.endswith(".asm"):
        return filecmp.cmp(a, b, shallow=False)
    with open(a, encoding="utf-8", errors="replace", newline="") as fa, open(b, encoding="utf-8", errors="replace", newline="") as fb:
        return canonical(fa.read()) == canonical(fb.read())


def compare(a, b, labels=False):
    names_a, names_b = set(os.listdir(a)), set(os.listdir(b))
    problems = [f"only in {a}: {n}" for n in sorted(names_a - names_b)]
    problems += [f"only in {b}: {n}" for n in sorted(names_b - names_a)]
    for n in sorted(names_a & names_b):
        if not same_file(os.path.join(a, n), os.path.join(b, n), labels):
            problems.append(f"differs: {n}")
    if problems:
        print("\n".join(problems))
        print(f"{len(problems)} difference(s)")
        return 1
    print(f"identical{' up to label numbers' if labels else ''} ({len(names_a)} files)")
    return 0


if __name__ == "__main__":
    if len(sys.argv) == 4 and sys.argv[1] == "take":
        sys.exit(take(sys.argv[2], sys.argv[3]))
    if len(sys.argv) == 4 and sys.argv[1] == "compare":
        sys.exit(compare(sys.argv[2], sys.argv[3]))
    if len(sys.argv) == 5 and sys.argv[1] == "compare" and sys.argv[2] == "--labels":
        sys.exit(compare(sys.argv[3], sys.argv[4], labels=True))
    sys.exit(__doc__)
