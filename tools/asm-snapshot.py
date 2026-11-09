#!/usr/bin/env python3
"""Takes and compares snapshots of the assembly the compiler writes for the repository's examples.

Used to check that a change meant to be internal does not change the output (spec `static-linking`).

    python tools/asm-snapshot.py take <zila-executable> <out-dir>
    python tools/asm-snapshot.py compare <dir-a> <dir-b>      (exit 1 when they differ)

A snapshot holds, for every `examples/*.zila` and `examples/packages/*.zila`, the output of `--asm`
for the default build, `--target linux` and `--no-opt` (`<file>.<variant>.asm`, and `.err` with what
the compiler printed to standard error). Run it from the repository root.
"""
import filecmp
import glob
import os
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


def compare(a, b):
    names_a, names_b = set(os.listdir(a)), set(os.listdir(b))
    problems = [f"only in {a}: {n}" for n in sorted(names_a - names_b)]
    problems += [f"only in {b}: {n}" for n in sorted(names_b - names_a)]
    for n in sorted(names_a & names_b):
        if not filecmp.cmp(os.path.join(a, n), os.path.join(b, n), shallow=False):
            problems.append(f"differs: {n}")
    if problems:
        print("\n".join(problems))
        print(f"{len(problems)} difference(s)")
        return 1
    print(f"identical ({len(names_a)} files)")
    return 0


if __name__ == "__main__":
    if len(sys.argv) == 4 and sys.argv[1] == "take":
        sys.exit(take(sys.argv[2], sys.argv[3]))
    if len(sys.argv) == 4 and sys.argv[1] == "compare":
        sys.exit(compare(sys.argv[2], sys.argv[3]))
    sys.exit(__doc__)
